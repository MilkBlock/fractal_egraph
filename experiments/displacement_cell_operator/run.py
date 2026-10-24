"""Correctness exploration, not a performance benchmark or production backend."""
from __future__ import annotations
import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import subprocess
import numpy as np
from model import (Graph, MatrixState, ScalarState, Operator, CoordinateOperator,
                   CoordinateState, OccurrenceState, InconsistentCoordinates, equality_pairs)

HERE = Path(__file__).resolve().parent


def fixture():
    # Three base fragments, two groupings of the same tuple, one opaque tuple.
    # 0:a, 1:b, 2:opaque[3], 3:ab, 4:b+opaque, 5:(ab)+opaque.
    # Grafting 5 := a+(b+opaque) adds an equivalent spine at a later boundary.
    return Graph([1,1,3,2,4,5], {0:0,1:1}, {2},
                 {3:[(0,1)],4:[(1,2)],5:[(3,2)]}, {0,1,3,4,5})


def apply_event(graph, event):
    if event['kind']=='length': graph.known_length.add(event['node'])
    elif event['kind']=='graft':
        graph.concat.setdefault(event['node'],[]).append(tuple(event['children']))
    else: raise ValueError(event)


def initial_egg(g, seeds):
    s = ['(datatype Expr (Leaf i64) (Opaque i64) (Single Expr) (Concat Expr Expr) (Get Expr i64))',
         '(function tuple-length (Expr) i64 :no-merge)',
         '(relation Add-Gets (Expr Expr i64))',
         '(relation Add-All-Gets (Expr Expr i64 i64))',
         '(relation Not-Just-Concat (Expr))', '(relation Label (i64 Expr))',
         '(relation Value (i64 Expr))', '(ruleset always-run)', HERE.joinpath('rules.egg').read_text()]
    for n in range(g.n):
        if n in g.single:
            term=f'(Single (Leaf {g.single[n]}))'
        elif n in g.opaque: term=f'(Opaque {n})'
        else:
            left,right=g.concat[n][0];term=f'(Concat n{left} n{right})'
        s += [f'(let n{n} {term})',f'(Label {n} n{n})']
        for l,r in g.concat.get(n,[])[1:]: s += [f'(union n{n} (Concat n{l} n{r}))']
        if n in g.known_length: s += [f'(set (tuple-length n{n}) {g.length[n]})']
        if n in g.opaque: s += [f'(Not-Just-Concat n{n})']
    for leaf in set(g.single.values()): s += [f'(Value {leaf} (Leaf {leaf}))']
    for n,i in seeds: s += [f'(Get n{n} {i})']
    return s


def event_egg(event,g):
    n=event['node']
    if event['kind']=='length': return f'(set (tuple-length n{n}) {g.length[n]})'
    l,r=event['children'];return f'(union n{n} (Concat n{l} n{r}))'


def observations(round):
    shapes = {
        'AG': (3, '((Label r orig) (Label n node) (Add-Gets orig node off))', 'r n off'),
        'ALL': (4, '((Label r orig) (Label n node) (Add-All-Gets orig node off p))', 'r n off p'),
        'GET': (2, '((Label n node) (Get node i))', 'n i'),
        'GG': (4, '((Label r orig) (Label n node) (= v (Get orig i)) (= v (Get node j)))','r i n j'),
        'GV': (3, '((Label r orig) (Value v leaf) (= leaf (Get orig i)))','r i v')}
    s=[f'(ruleset observe{round})']
    for name,(arity,lhs,args) in shapes.items():
        s += [f'(relation {name}{round} ('+ ' '.join(['i64']*arity) +'))',
              f'(rule {lhs} (({name}{round} {args})) :ruleset observe{round})']
    s += [f'(run observe{round} 1)']
    s += [f'(print-function {name}{round} 100000000)' for name in shapes]
    return s


def expected_rows(state):
    a,b,gets=state.snapshot()
    pairs=equality_pairs(gets,state.unions,set(state.op.graph.single.values()))
    return {'AG':a,'ALL':b,'GET':gets,
            'GG':{u[1:]+v[1:] for u,v in pairs if u[0]==v[0]=='g'},
            'GV':{u[1:]+v[1:] for u,v in pairs if u[0]=='g' and v[0]=='v'}}


def native_check(binary, graph, seeds, events, rounds, out, worker=None):
    out.mkdir(parents=True,exist_ok=False)
    state=MatrixState(graph,seeds);scalar=ScalarState(graph,seeds)
    try: coordinate=CoordinateState(graph,seeds)
    except InconsistentCoordinates: coordinate=OccurrenceState(graph,seeds)
    source=initial_egg(graph,seeds)
    expected={};records=[];rank_updates=[]
    slot_nodes=(list(range(graph.n)) if isinstance(coordinate,CoordinateState) else coordinate.op.nodes.tolist())
    seed_slots=(list(range(graph.n)) if isinstance(coordinate,CoordinateState)
                else [coordinate.op.index[(n,0)] for n in range(graph.n)])
    gpu_job={'n':graph.n,'slots':coordinate.a.shape[1],'width':graph.width,
             'nodes':slot_nodes,'seeds':seed_slots,'lengths':[graph.length[n] for n in slot_nodes],
             'single':[int(n in graph.single) for n in slot_nodes],
             'opaque':[int(n in graph.opaque) for n in slot_nodes],
             'gets':coordinate.gets.astype(int).ravel().tolist(),'epochs':[],'steps':[]}
    gpu_expected=[]
    for r in range(1,rounds+1):
        for event in events.get(r,[]):
            changed=copy.deepcopy(state.op.graph);apply_event(changed,event)
            source += [event_egg(event,changed)]
            rank_updates += state.op.update(changed)
            coordinate.op.update(changed)
            np.testing.assert_array_equal(coordinate.op.k,CoordinateOperator(changed).k)
            scalar.graph=copy.deepcopy(changed)
            rebuilt=Operator(changed)
            np.testing.assert_allclose(state.op.k,rebuilt.k,rtol=1e-12,atol=1e-10)
        current=coordinate.op.graph
        h=(coordinate.op.h.tolist() if isinstance(coordinate,CoordinateState) else coordinate.op.offsets.tolist())
        root_h=(h if isinstance(coordinate,CoordinateState) else [0]*graph.n)
        epoch={'t':coordinate.op.t.ravel().tolist(),'h':h,'root_h':root_h,
               'known':[int(n in current.known_length) for n in slot_nodes]}
        if not gpu_job['epochs'] or epoch!=gpu_job['epochs'][-1]:gpu_job['epochs'].append(epoch)
        gpu_job['steps'].append(len(gpu_job['epochs'])-1)
        se=((coordinate.a>0)&np.array(gpu_job['single'])[None,:]).astype(int).ravel().tolist()
        oe=((coordinate.frontier>=0)&np.array(epoch['known'])[None,:]
            &(coordinate.frontier<np.array(gpu_job['lengths'])[None,:])).astype(int).ravel().tolist()
        state.step();scalar.step();coordinate.step()
        assert state.snapshot()==scalar.snapshot(),f'scalar state mismatch at round {r}'
        assert state.snapshot()==coordinate.snapshot(),f'coordinate state mismatch at round {r}'
        assert expected_rows(state)==expected_rows(coordinate)
        # Different derivations may emit repeated edges. Compare equality, not
        # action count or hashcons writes, across the output boundary.
        assert equality_pairs(scalar.gets,scalar.unions,set(graph.single.values())) == \
               equality_pairs(state.snapshot()[2],state.unions,set(graph.single.values()))
        source += ['(run always-run 1)']+observations(r)
        expected[r]=expected_rows(state)
        records.append({'round':r,**{k:len(v) for k,v in expected[r].items()}})
        gpu_expected.append({'a':coordinate.a.ravel().tolist(),
            'frontier':coordinate.frontier.ravel().tolist(),'gets':coordinate.gets.astype(int).ravel().tolist(),
            'single_effect':se,'opaque_effect':oe})
    program=out/'native.egg';program.write_text('\n'.join(source)+'\n')
    result=subprocess.run([str(binary),str(program)],capture_output=True,text=True,check=True)
    (out/'native.stdout').write_text(result.stdout);(out/'native.stderr').write_text(result.stderr)
    actual={r:{k:set() for k in ('AG','ALL','GET','GG','GV')} for r in expected}
    for m in re.finditer(r'\((AG|ALL|GET|GG|GV)(\d+) ((?:-?\d+\s*)+)\)\s*->',result.stdout):
        name,r,values=m.groups();actual[int(r)][name].add(tuple(map(int,values.split())))
    for r,rows in expected.items():
        for kind,want in rows.items():
            got=actual[r][kind]
            if got!=want:
                raise AssertionError(f'native round {r} {kind}: missing={sorted(want-got)[:12]}, extra={sorted(got-want)[:12]}')
    # One-shot closure is only offered as saturation, NEVER substituted for Run 1.
    seed=np.zeros_like(state.a)
    roots=np.flatnonzero(np.any(state.gets,axis=1));seed[roots,roots,0]=True
    saturated=state.op.closure(seed)
    # Continue exact one-round machine until all state stops changing, no budget.
    while True:
        before=state.snapshot(),state.unions.copy()
        state.step()
        if before==(state.snapshot(),state.unions): break
    assert np.array_equal(saturated,state.a)
    gpu=None
    if worker:
        job_path=out/'gpu-job.json';job_path.write_text(json.dumps(gpu_job))
        gpu_path=out/'gpu-output.json'
        subprocess.run([str(worker.resolve()),str(job_path),str(gpu_path)],check=True)
        gpu=json.loads(gpu_path.read_text())
        for r,(got,want) in enumerate(zip(gpu['rounds'],gpu_expected,strict=True),1):
            for key in want:
                np.testing.assert_array_equal(got[key],want[key],err_msg=f'GPU round {r} {key}')
        gpu={k:v for k,v in gpu.items() if k!='rounds'}
        gpu.update({'exact_all_rounds_and_union_packets':True,'epoch_uploads':len(gpu_job['epochs']),
                    'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest()})
    return {'rounds':records,'native_exact':True,'scalar_exact':True,'gpu':gpu,
            'rank_update_edges':len(rank_updates),'inverse_residual':state.op.decode_error,
            'state_bytes':state.a.nbytes+state.frontier.nbytes+state.gets.nbytes,
            'operator_bytes':state.op.t.nbytes+state.op.k.nbytes,
            'coordinate_state_bytes':coordinate.a.nbytes+coordinate.frontier.nbytes+coordinate.gets.nbytes,
            'coordinate_operator_bytes':coordinate.op.t.nbytes+coordinate.op.k.nbytes,
            'coordinate_kind':type(coordinate).__name__,
            'coordinate_exact':True,
            'saturated_AG':int(state.a.sum()),
            'frontier_coordinates':int((state.frontier>=0).sum()),
            'expanded_ALL_rows':len(state.snapshot()[1]),
            'source_sha256':hashlib.sha256(program.read_bytes()).hexdigest()}


def scale_graph(leaves):
    # Shared prefix tuples, with BOTH left-associated and suffix groupings.
    # Facts are immutable tuples; many paths lead to the same output position.
    g=Graph([1]*leaves,{i:i for i in range(leaves)},set(),{},set(range(leaves)))
    intervals={(i,i+1):i for i in range(leaves)}
    for width in range(2,leaves+1):
        for start in range(leaves-width+1):
            end=start+width;n=g.n;g.length.append(width);g.known_length.add(n)
            splits=sorted({start+1,end-1})
            g.concat[n]=[(intervals[start,k],intervals[k,end]) for k in splits]
            intervals[start,end]=n
    return g


def inspect_operator(g):
    op=Operator(g)
    coefficients,error=integral_coefficients(op)
    reach=np.any(coefficients>0,axis=0)
    # Restrict both axes to the union of semantically used coordinates only;
    # report full density as well, never call padding dense useful work.
    quotient=CoordinateOperator(g)
    d=np.exp(-2j*np.pi*np.arange(g.width)[:,None]*quotient.h[None,:]/g.width)
    reconstructed=d.conj()[:,:,None]*quotient.t[None,:,:]*d[:,None,:]
    np.testing.assert_allclose(op.t,reconstructed,rtol=1e-12,atol=1e-12)
    np.testing.assert_array_equal(reach,quotient.k>0)
    leaf_nodes=list(g.single)
    boundary=quotient.k[:,leaf_nodes]
    wide=np.array(g.length)>=max(g.length)/2
    return {'nodes':g.n,'offset_coordinates':g.width,'edges':len(g.edges()),
            'transition_nonzero_fraction':len({(a,b) for a,b,_ in g.edges()})/g.n**2,
            'closure_reachable_fraction':float(reach.mean()),
            'max_path_multiplicity':int(coefficients.max()),
            'spectral_operator_bytes':op.k.nbytes,
            'coordinate_closure_bytes':quotient.k.nbytes+quotient.h.nbytes,
            'boundary_transfer_shape':list(boundary.shape),
            'boundary_transfer_density':float(boundary.mean()),
            'wide_root_boundary_density':float(boundary[wide].mean()),
            'explicit_offset_operator_bytes_float64':(g.n*g.width)**2*8,
            'numeric_inverse_residual':error}


def integral_coefficients(op):
    from model import integral_inverse
    return integral_inverse(op.k)


def main():
    p=argparse.ArgumentParser();p.add_argument('--native',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--survey',type=Path);p.add_argument('--gpu',type=Path);args=p.parse_args()
    args.output.mkdir(parents=True,exist_ok=False)
    base=fixture()
    result={'kind':'standalone CPU NumPy representation experiment; native correctness oracle; no speedup claim',
            'native_binary':str(args.native.resolve()),
            'native_sha256':hashlib.sha256(args.native.read_bytes()).hexdigest(),
            'rules_sha256':hashlib.sha256((HERE/'rules.egg').read_bytes()).hexdigest(),
            'cases':{},'scales':[]}
    late=copy.deepcopy(base);late.known_length.remove(3)
    result['cases']['late_length_and_graft']=native_check(args.native,late,[(5,0)],
            {3:[{'kind':'length','node':3}],5:[{'kind':'length','node':2}],
             8:[{'kind':'graft','node':5,'children':[0,4]}]},16,args.output/'late',args.gpu)
    all_known=copy.deepcopy(base);all_known.known_length=set(range(base.n))
    result['cases']['initial_known']=native_check(args.native,all_known,[(4,0),(5,0)],{},12,args.output/'known',args.gpu)
    for n in (4,8,12): result['scales'].append({'base_leaves':n,**inspect_operator(scale_graph(n))})
    if args.survey:
        survey=json.loads(args.survey.read_text());result['real_snapshot_survey']=survey['counts']
        for i,s in enumerate(survey['replay_slices']):
            graph=Graph(s['length'],{int(k):v for k,v in s['single'].items()},set(s['opaque']),
                        {int(k):[tuple(v) for v in vals] for k,vals in s['concat'].items()},set(s['known_length']))
            rounds=max(graph.length)+4
            key=f'raytrace_root_{s["original_root"]}'
            result['cases'][key]=native_check(args.native,graph,[(s['root'],0)],{},rounds,args.output/key,args.gpu)
            result['cases'][key]['operator']=inspect_operator(graph)
    # The same shared subtree at two positions must not be collapsed to one
    # coordinate. The general displacement representation retains BOTH paths.
    repeated=Graph([1,2],{0:0},set(),{1:[(0,0)]},{0,1})
    try:
        CoordinateOperator(repeated)
        raise AssertionError('expected inconsistent repeated-occurrence coordinates')
    except InconsistentCoordinates as e:
        general=Operator(repeated);seed=np.zeros((2,2,3),dtype=bool);seed[1,1,0]=True
        response=general.closure(seed)
        assert set(np.flatnonzero(response[1,0]))=={0,1}
        result['repeated_occurrence']={'coordinate_rejected':str(e),
            'general_offsets':list(map(int,np.flatnonzero(response[1,0])))}
    result['cases']['repeated_subtree']=native_check(args.native,repeated,[(1,0)],{},7,args.output/'repeated',args.gpu)
    (args.output/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__=='__main__': main()

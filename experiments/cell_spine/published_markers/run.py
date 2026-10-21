"""Small native lowering/selection experiment, not an end-to-end eggcc benchmark."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from tools.cell_spine import interfaces,markers,takeover as t,solver
from tools.cell_spine.pricing import RuntimePricing
from tools.bench.math_graph_equal import verify


def main():
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True)
    p.add_argument('--copies',type=int,default=128);args=p.parse_args()
    out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    sig={op:dict(input=['T']*n,output='T',kind='constructor') for op,n in [('A',1),('B',2)]}
    sig['Leaf']=dict(input=['i64'],output='T',kind='constructor')
    root=t.call('B',[t.call('A',[t.var('x')]),t.var('y')])
    c=dict(id=0,family=0,roots=[root],params={'x':'p0','y':'p1'},param_sorts={'x':'T','y':'T'},signatures=sig,acts=[])
    c['marker_layouts']=markers.layouts(c)[:1] # Controlled ABI ablation, not the general compiler's candidate domain.
    logical=t.call('Summary-0!',list(map(t.var,['r0','p0','p1'])))
    body=[dict(kind='fact',expr=logical)]
    consumers=[dict(name=f'use-{i}',ruleset='Run!',body=body,head=[dict(kind='expr',expr=t.call(f'Out{i}!',
                    [t.call('A!',[t.var('p0')]),t.var('p1')]))]) for i in range(12)]
    birth=dict(name='birth',ruleset='Birth',body=[dict(kind='eq',args=[t.var('r0'),
                t.call('B!',[t.call('A!',[t.var('p0')]),t.var('p1')])])],head=[dict(kind='expr',expr=logical)])
    bundle=interfaces.extend(dict(chosen={0:c},consumers=[birth]+consumers),'publisher')
    common='''(datatype T! (Leaf! i64) (A! T!) (B! T! T!) (A!! T!) (B!! T! T!)
'''+ '\n'.join(f'(Out{i}! T! T!)' for i in range(12))+')\n(ruleset Birth) (ruleset Run!)\n(sort Cell!)'
    initial='\n'.join(f'(B! (A! (Leaf! {2*i})) (Leaf! {2*i+1}))' for i in range(args.copies))
    (out/'input.egg').write_text(initial)
    schedule='(run-schedule (saturate (seq Birth Run!)))'
    runner=ROOT/'target/release/examples/native_math_baseline';results={};cache={}
    def execute(layout):
        key=layout['id']
        if key in cache:return cache[key]
        selected={0:layout}
        # All columns in this fixture have the one equality sort.
        declarations=f'(constructor Summary-0! ({" ".join(["T!"]*layout["columns"])}) Cell!)'
        env='\n'.join([common,declarations]+interfaces.declarations(selected,sig)+
                      [markers.lower(r,selected,sig) for r in bundle['consumers']])
        (out/f'{key}-env.egg').write_text(env)
        (out/f'{key}.egg').write_text(env+'\n'+initial+'\n'+schedule+'\n')
        request=dict(environment=str(out/f'{key}-env.egg'),input=str(out/'input.egg'),run=schedule,
                     constructors=['Leaf!','A!','B!','A!!','B!!']+[f'Out{i}!' for i in range(12)])
        path=out/f'{key}.json';path.write_text(json.dumps(request))
        r=subprocess.run([runner,'bench-probes',path,out/f'{key}-graph.json'],capture_output=True,text=True,check=True)
        report=json.loads(r.stdout.splitlines()[-1]);counts=report['verification']['run_probe_counters']
        results[key]=dict(probes=counts['probes'],candidate_rows=counts['candidate_rows'],
                          logical_matches=report['matches'],instrumented_run_seconds=report['run_seconds'])
        cache[key]=dict(value=counts['probes']+counts['candidate_rows'],counters=counts)
        return cache[key]
    solver.select('scip')
    pricing=RuntimePricing(lambda *_:bundle,sig,out/'pricing',(1,1,1),search=lambda b,s,f:execute(s[0]))
    _,optimum=pricing.optimize(dict(selected=[0],implementations=[]))
    for layout in bundle['chosen'][0]['marker_layouts']:execute(layout)
    equality={key:verify(out/'m0_0-graph.json',out/f'{key}-graph.json',{'T!'}) for key in cache}
    (out/'results.json').write_text(json.dumps(dict(scope='Actual native execution of generated marker ABIs; controlled lowering fixture, not Analyze coverage or pristine eggcc speedup',
        copies=args.copies,consumers=12,results=results,selected=optimum['marker_layouts'],
        binary_sha256=hashlib.sha256(runner.read_bytes()).hexdigest(),
        generated_sha256={k:hashlib.sha256((out/f'{k}.egg').read_bytes()).hexdigest() for k in cache},
        equality=equality,solver=solver.statistics()),indent=2)+'\n')
    print(json.dumps(dict(results=results,selected=optimum['marker_layouts'],equality=equality)))


if __name__=='__main__':main()

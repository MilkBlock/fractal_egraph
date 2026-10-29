"""Batch proof obligations + fresh-process per-Run GPU reproducibility.

No whole-program CPU endpoint equivalence requirement. Positive fixtures have
independent arithmetic/absence checks; source-rule validation is always active
in the GPU block publisher and exact numeric audit is enabled here.
"""
import argparse,copy,json,sys,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
from test_generic_cells import core_for
from run_generic_cells import run_library
from eggfuse.analyzer.regions import from_core
from eggfuse.engine.regions import verify_repeat,execution_summary
from eggfuse.ir.recipe import build,digest
from eggfuse.ir.batches import validate_batch


def rows(result,folder,table):
    p=json.loads((folder/'program-0/cell-0/program.json').read_text());i=result['programs'][0]['instances'][0]
    tid=next(j for j,t in enumerate(p['tables']) if t['name']==table)
    width=len(p['tables'][tid]['schema']);return [r[4:4+width] for r in i['rows'] if r[0]==tid and r[1]==0]


def check(build_dir,out,only=None,real_dir=None):
    out.mkdir(parents=True,exist_ok=False);base=(ROOT/'experiments/staged_batches/chain.egg').read_text().split('(Pair (N 0)')[0]
    schedule='(run-schedule (saturate (run build) (run advance) (run finish)))'
    wide=base+''.join(f'(Pair (N {2*i}) (N {2*i+1})) (set (Cost (N {2*i})) {i%100-50}) (set (Cost (N {2*i+1})) {i%80-40})\n' for i in range(513))+schedule
    cases={
      'wide':wide,
      'contended-key':base.replace('(set (Best r) n)','(set (Best (N 0)) n)')+''.join(f'(Pair (N {2*i}) (N {2*i+1})) (set (Cost (N {2*i})) {i}) (set (Cost (N {2*i+1})) 3)\n' for i in range(17))+schedule,
      'min-column':base.replace('(+ 1 (+ a b))','(min a b)')+'(Pair (N 0) (N 1)) (set (Cost (N 0)) -7) (set (Cost (N 1)) 3)'+schedule,
      'large-i64':base+'(Pair (N 0) (N 1)) (set (Cost (N 0)) 10000000000000000) (set (Cost (N 1)) 3)'+schedule,
      'finite':base+'(Pair (N 0) (N 1)) (set (Cost (N 0)) 3) (set (Cost (N 1)) 4) (run build 1)',
      'lost-min':'''(datatype E (N i64)) (function Best (E) i64 :merge (min old new))
(relation Offer (E i64)) (relation Candidate (E i64)) (relation Wrong (E i64))
(ruleset a) (ruleset b)
(rule ((Offer r n)) ((set (Best r) n) (Candidate r n)) :ruleset a)
(rule ((= n (Best r)) (> n 0)) ((Wrong r n)) :ruleset b)
(Offer (N 0) 5) (set (Best (N 0)) 0)
(run-schedule (saturate (run a) (run b)))''',
      'bound-guard':'''(datatype E (N i64)) (relation Seed (E i64 i64)) (relation Mid (E i64)) (relation Wrong (E i64))
(ruleset a) (ruleset b)
(rule ((Seed r x y)) ((Mid r x)) :ruleset a)
(rule ((Seed r x y) (Mid r z) (= z (+ x y))) ((Wrong r z)) :ruleset b)
(Seed (N 0) 5 1) (run-schedule (saturate (run a) (run b)))''',
      'overflow-merge':'''(datatype E (N i64)) (relation Seed (E i64))
(function Total (E) i64 :merge (+ old new)) (ruleset a)
(rule ((Seed x n)) ((set (Total x) n)) :ruleset a)
(Seed (N 0) 1) (set (Total (N 0)) 9223372036854775807)
(run-schedule (saturate (run a)))''',
      'masked-overflow':'''(datatype E (N i64)) (relation Seed (E i64)) (relation Mid (E i64)) (relation Wrong (E i64))
(ruleset a) (ruleset b)
(rule ((Seed r n)) ((Mid r n)) :ruleset a)
(rule ((Mid r n) (< n 0)) ((Wrong r (+ n 1))) :ruleset b)
(Seed (N 0) 9223372036854775807) (run-schedule (saturate (run a) (run b)))''',
      'constructor-resume':'''(datatype E (N i64) (Edge E)) (relation Seed (E E)) (relation Seen (E)) (relation Done (E))
(ruleset a) (ruleset b) (ruleset c)
(rule ((Seed x y)) ((union (Edge x) y)) :ruleset a)
(rule ((= y (Edge x))) ((Seen y)) :ruleset b)
(rule ((Seen y)) ((Done y)) :ruleset c)
(Seed (N 0) (N 2)) (Edge (N 0))
(run-schedule (saturate (run a) (run b) (run c)))''',
      'compacting-return':'''(datatype E (N i64) (K E) (Box E) (Gate E)) (relation Seed (E E E)) (relation Seen (E E))
(ruleset a) (ruleset b)
(rule ((Seed x y w)) ((K x) (K y) (Box w) (union x y) (union (Gate w) w)) :ruleset a)
(rule ((= v (Gate w))) ((Seen v (Box w))) :ruleset b)
(Seed (N 0) (N 1) (N 2)) (Gate (N 2))
(run-schedule (saturate (run a) (run b)))''',
      'retired-continuation':'''(datatype E (N i64) (Edge E)) (relation Seed (E E)) (relation Kill (E)) (relation Done (E))
(ruleset a) (ruleset b)
(rule ((Seed x y)) ((union (Edge x) y)) :ruleset a)
(rule ((Kill x)) ((subsume (Edge x))) :ruleset a)
(rule ((= y (Edge x))) ((Done y)) :ruleset b)
(Seed (N 0) (N 2)) (Kill (N 0)) (Edge (N 0))
(run-schedule (saturate (run a) (run b)))''',
      'shared-identity':'''(datatype E (N i64) (Shared E)) (relation Seed (E i64)) (relation Seen (E E)) (relation Done (E))
(ruleset a) (ruleset b)
(rule ((Seed x n)) ((Seen x (Shared x))) :ruleset a)
(rule ((Seen x y)) ((Done y)) :ruleset b)
(Seed (N 0) 1) (Seed (N 0) 2)
(run-schedule (saturate (run a) (run b)))''',
      'actual-min-port':'''(datatype E (N i64)) (relation Seed (E i64)) (function Best (E) i64 :merge (min old new)) (relation Done (E i64))
(ruleset a) (ruleset b)
(rule ((Seed x n)) ((set (Best x) n)) :ruleset a)
(rule ((= n (Best x))) ((Done x (+ n 1))) :ruleset b)
(Seed (N 0) 12) (set (Best (N 0)) 3)
(run-schedule (saturate (run a) (run b)))''',
      'custom-chain':'''(datatype E (N i64) (Choice E E)) (relation Seed (E E)) (function F (E) E :merge (Choice old new)) (relation Done (E E))
(ruleset a) (ruleset b)
(rule ((Seed x y) (= old (F x))) ((set (F x) y)) :ruleset a)
(rule ((= c (Choice left right))) ((union c right) (Done left right)) :ruleset b)
(Seed (N 0) (N 1)) (set (F (N 0)) (N 0))
(run-schedule (saturate (run a) (run b)))''',
      'custom-merge':'''(datatype E (N i64) (Choice E E)) (relation Seed (E E E)) (function F (E) E :merge (Choice old new)) (relation Done (E E))
(ruleset a) (ruleset b)
(rule ((Seed x y z)) ((set (F x) y) (set (F x) z)) :ruleset a)
(rule ((= c (Choice left right))) ((Done left right)) :ruleset b)
(Seed (N 0) (N 1) (N 2)) (Seed (N 0) (N 3) (N 4))
(run-schedule (saturate (run a) (run b)))''',
      'union-and-subsume':'''(datatype E (N i64) (Edge E E)) (relation Seen (E)) (relation Done (E))
(ruleset observe) (ruleset edit)
(rule ((= e (Edge x y))) ((Seen e)) :ruleset observe)
(rule ((Seen e)) ((Done e)) :ruleset observe)
(rule ((= e (Edge x y))) ((union x y) (subsume (Edge x y))) :ruleset edit)
(Edge (N 0) (N 1)) (run-schedule (saturate (run observe) (run edit)))'''
    }
    cases['independent-unions']=cases['constructor-resume'].replace('(Seed (N 0) (N 2)) (Edge (N 0))', ''.join(f'(Seed (N {3*i}) (N {3*i+2})) (Edge (N {3*i}))' for i in range(129)))
    cases['set-key-merge']='''(sort E) (constructor N (i64) E)
(sort S (Set E)) (constructor Box (S) E)
(relation Link (E E)) (relation Seen (E)) (ruleset a) (ruleset b)
(rule ((Link x y)) ((union x y)) :ruleset a)
(rule ((= w (Box s))) ((Seen w)) :ruleset b)
'''+''.join(f'(Link (N {2*i}) (N {2*i+1})) (Box (set-insert (set-insert (set-empty) (N {2*i})) (N {2*i+1}))) (Box (set-insert (set-empty) (N {2*i})))\n' for i in range(129))+'(run-schedule (saturate (run a) (run b)))'
    if real_dir:
        for name in ['finite','late-union','context-to-terms','renamed']:
            cases['real-'+name]=(real_dir/(name+'.egg')).read_text()
    if only:
        missing=set(only)-set(cases)
        if missing:raise ValueError('unknown fixture '+str(missing))
        cases={k:v for k,v in cases.items() if k in only}
    results={}
    for name,text in cases.items():
        source=out/(name+'.egg');source.write_text(text);core=core_for(source,build_dir);plan=from_core(core)
        validate_batch(plan['batch_program'],build(core),plan)
        if name=='wide':
            bad=copy.deepcopy(plan['batch_program']);t=next(t for t in bad['templates'] if len(t['steps'])==3)
            step=t['steps'][0];r=build(core)['rules'][step['recipe_rule']]
            dst=next(i for i,s in enumerate(r['slots']) if s['definition']!='query' and s['sort']['kind']=='i64')
            input_slot=next(i for i,s in enumerate(t['slots'][:t['input_width']]) if s['sort']['kind']=='i64')
            step['mapping'][dst]={'slot':input_slot};bad['sha256']=digest({k:v for k,v in bad.items() if k!='sha256'})
            try:validate_batch(bad,build(core),plan)
            except ValueError:pass
            else:raise AssertionError('SSA overwrite accepted')
        initial=core.pop('initial');lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
        runs=[run_library(lib,build_dir,out/(name+'-'+suffix),capacity=8192,match_capacity=4096,region_program=plan,audit_numeric=True) for suffix in ['first','repeat']]
        if name=='overflow-merge':
            states=[r['programs'][0]['instances'][0] for r in runs]
            assert states[0]==states[1] and states[0]['meta'][4]==5
            native=subprocess.run([str(build_dir/'frontend'),str(source),'--native-full','--export-to',str(out/'native-overflow')],text=True,capture_output=True)
            assert native.returncode!=0
            (out/'native-overflow.log').write_text(native.stdout+native.stderr)
            results[name]=dict(expected_source_error='i64 merge overflow',device_error=5,native_failed=True,exact_repeated_private_state=True)
            (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');print(name,'expected error and repeated state verified',flush=True)
            continue
        verified=verify_repeat(*runs);summary=execution_summary(runs[0]);folder=out/(name+'-first')
        if name=='wide':
            done=rows(runs[0],folder,'Done');expected=sorted(i%100+i%80-87 for i in range(513))
            assert sorted(r[1] for r in done)==expected
            assert summary['validated_successor_invocations']>=1026 and summary['matrix_context_rows']>=513
            assert summary['max_tile_rows']==128
            assert summary['completed_binding_shortcuts']>=1026
            assert summary['batch_seed_invocations']==513
            assert any(a['batch_context_growths']>0 for a in runs[0]['programs'][0]['attempts'])
        if name=='contended-key':assert [r[1] for r in rows(runs[0],folder,'Best')]==[6]
        if name=='min-column':assert [r[1] for r in rows(runs[0],folder,'Done')]==[-5]
        if name=='large-i64':
            assert [r[1] for r in rows(runs[0],folder,'Middle')]==[10000000000000004]
            assert not rows(runs[0],folder,'Done') and summary['matrix_context_rows']==0
        if name=='finite':assert summary['validated_successor_invocations']==0
        if name in ['lost-min','bound-guard','masked-overflow']:assert not rows(runs[0],folder,'Wrong')
        if name=='lost-min':assert summary['rejected_successor_invocations']>0
        if name=='independent-unions':
            assert len(rows(runs[0],folder,'Done'))==129
            assert summary['continuation_blocks']<=2 and summary['validated_successor_invocations']==258
        if name=='constructor-resume':
            assert rows(runs[0],folder,'Done')
            assert summary['continuation_blocks']>0 and summary['constructor_set_requests']>0
            assert summary['validated_successor_invocations']>0
        if name=='compacting-return':
            box=rows(runs[0],folder,'Box')[0][-1]
            assert all(r[1]==box for r in rows(runs[0],folder,'Seen')), 'constructor prediction survived row compaction with wrong identity'
        if name=='retired-continuation':
            assert not rows(runs[0],folder,'Done')
            assert summary['rejected_successor_invocations']>0
        if name=='shared-identity':
            assert len(rows(runs[0],folder,'Shared'))==1 and len(rows(runs[0],folder,'Done'))==1
        if name=='actual-min-port':
            assert [r[1] for r in rows(runs[0],folder,'Done')]==[4]
            assert summary['validated_successor_invocations']>0
        if name=='custom-chain':
            assert summary['validated_successor_invocations']>0 and summary['union_merge_blocks']>0
        if name=='custom-merge':
            assert len(rows(runs[0],folder,'Choice'))==3
            assert summary['custom_merge_requests']>=4
            assert len(rows(runs[0],folder,'Done'))==3 # every correlated callback, not just a winner
        if name=='set-key-merge':
            assert len(rows(runs[0],folder,'Box'))==129
            assert len(rows(runs[0],folder,'Seen'))==129
        results[name]=dict(verification=verified,execution=summary,templates=len(plan['batch_program']['templates']))
        (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');print(name,summary,flush=True)
    return results

def check_sort(build_dir,out):
    """Same runtime, source bindings/effects and total order; only sorting differs."""
    import run_generic_cells as runtime
    out.mkdir(parents=True,exist_ok=False);results={};original=runtime.lower
    for count in [0,1,255,256,257,513,4097,32768]:
        source=out/f'bindings-{count}.egg'
        source.write_text('''(datatype E (N i64)) (N 0)
(relation Seed (i64 i64)) (relation Seen (i64 i64)) (relation Final (i64 i64))
(function Pick (i64) i64 :merge new) (set (Pick 0) -1)
(ruleset s) (ruleset t)
(rule ((Seed x y)) ((set (Pick 0) (+ x y)) (Seen x y)) :ruleset s)
(rule ((Seen x y)) ((Final x y)) :ruleset t)
'''+''.join(f'(Seed {i} {(37*i+11)%997})\n' for i in range(count))+
            '(run-schedule (saturate (run s) (run t)))')
        core=core_for(source,build_dir);plan=from_core(core);initial=core.pop('initial')
        lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
        reports={}
        for enabled in [False,True]:
            def lower(*args,**kwargs):return original(*args,**kwargs,parallel_sort=enabled)
            runtime.lower=lower
            try:reports[enabled]=runtime.run_library(lib,build_dir,out/f'{count}-{enabled}',capacity=max(512,4*count+64),match_capacity=max(19,count+19),region_program=plan,audit_numeric=True)
            finally:runtime.lower=original
        verified=verify_repeat(reports[False],reports[True])
        folder=out/f'{count}-True';r=reports[True];s=r['programs'][0]['instances'][0]
        expected={(i,(37*i+11)%997) for i in range(count)}
        assert {tuple(row[:2]) for row in rows(r,folder,'Seen')}==expected
        assert {tuple(row[:2]) for row in rows(r,folder,'Final')}==expected
        # merge-new exposes binding order, not just the set of produced rows.
        p=json.loads((folder/'program-0/attempt-0/program.json').read_text())
        tid=next(i for i,t in enumerate(p['tables']) if t['name']=='Seed')
        flat=p['instances'][0]['initial_rows'];seed=[flat[i:i+p['stride']] for i in range(0,len(flat),p['stride']) if flat[i]==tid]
        final=max(seed,key=lambda row:row[3]) if seed else None
        assert rows(r,folder,'Pick')[0][1]==(final[4]+final[5] if final else -1)
        results[count]=dict(verification=verified,bindings=count,matched=sum(h[2] for h in s['history']),
            semantic_scope='actual resident multi-stage Metal engine, unchanged matching and effects; executor sorting credit only',
            timing_scope='single checked diagnostic per variant; not full Pass-3 speedup',
            seconds={str(k):dict(gpu=v['programs'][0]['gpu_seconds'],inference=v['programs'][0]['inference_seconds']) for k,v in reports.items()})
        (out/'sort-results.json').write_text(json.dumps(results,indent=2)+'\n')
        print(count,results[count]['seconds'],flush=True)
    # Independent arenas can pause at different positions within a Run. A large
    # neighbor's growth must not erase a small arena's pending union continuation.
    prefix='''(datatype E (N i64) (Edge E)) (relation Seed (E E)) (relation Done (E))
(ruleset a) (ruleset b)
(rule ((Seed x y)) ((union (Edge x) y)) :ruleset a)
(rule ((= y (Edge x))) ((Done y)) :ruleset b)
'''
    cores=[]
    for count in [1,129]:
        source=out/f'arena-{count}.egg'
        source.write_text(prefix+''.join(f'(Seed (N {3*i}) (N {3*i+2})) (Edge (N {3*i}))\n' for i in range(count))+'(run-schedule (saturate (run a) (run b)))')
        cores.append(core_for(source,build_dir))
    plan=from_core(cores[0]);base=dict(cores[0]);base.pop('initial')
    lib=dict(schema='analyzer-device-cells/v1',programs=[base],instances=[dict(unit=i,program=0,initial=c['initial']) for i,c in enumerate(cores)])
    private=run_library({**lib,'instances':lib['instances'][:1]},build_dir,out/'private-continuation',4096,1024,quantum=1,region_program=plan,audit_numeric=True)
    shared=run_library(lib,build_dir,out/'shared-continuation',4096,1024,quantum=1,region_program=plan,audit_numeric=True)
    assert private['complete'] and shared['complete']
    a=private['programs'][0]['instances'][0];b=shared['programs'][0]['instances'][0]
    for key in a:
        if key=='batch_statistics':assert [x for j,x in enumerate(a[key]) if j not in (9,24)]==[x for j,x in enumerate(b[key]) if j not in (9,24)],key
        else:assert a[key]==b[key],key
    assert a['batch_statistics'][16]>0
    results['neighbor-growth']=dict(continuation_retained=True,full_small_arena_state_and_history_equal=True)
    (out/'sort-results.json').write_text(json.dumps(results,indent=2)+'\n')
    return results


def check_pipeline_cache(build_dir,out,program):
    """A persisted pipeline must reproduce the same effects, including corruption recovery."""
    import os
    from run_generic_cells import build_worker
    out.mkdir(parents=True,exist_ok=False);worker=build_worker(build_dir)
    expected=json.loads(program.with_name('result.json').read_text())['instances']
    cache=out/'archive';results={}
    for mode in ['off','cold','warm','corrupt','changed-source']:
        use=program
        if mode=='corrupt':next(cache.glob('*.metalarc')).write_bytes(b'invalid archive regression')
        if mode=='changed-source':
            p=json.loads(program.read_text());p['shader']+='\n// cache key isolation regression\n'
            use=out/'changed-program.json';use.write_text(json.dumps(p))
        env={**os.environ,'EGGFUSE_METAL_CACHE':'off' if mode=='off' else str(cache)}
        with (out/(mode+'.log')).open('w') as log:
            subprocess.run([str(worker),str(use),str(out/(mode+'.json'))],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        r=json.loads((out/(mode+'.json')).read_text());assert r['instances']==expected
        results[mode]={k:r[k] for k in ['shader_compile_seconds','library_compile_seconds','pipeline_compile_seconds','pipeline_cache']}
        if mode=='warm':assert r['pipeline_cache']['hits']==len(r['pipeline_compile_seconds']) and r['pipeline_cache']['misses']==0
        if mode=='corrupt':assert r['pipeline_cache']['warnings']
        if mode=='changed-source':assert r['pipeline_cache']['key']!=results['warm']['pipeline_cache']['key'] and r['pipeline_cache']['hits']==0
    results['full_states_and_histories_equal']=True
    (out/'cache-results.json').write_text(json.dumps(results,indent=2)+'\n');return results


def check_indexes(build_dir,out):
    import run_generic_cells as runtime
    out.mkdir(parents=True,exist_ok=False);source=out/'indexed-unions.egg'
    source.write_text('''(datatype E (N i64)) (relation Link (i64 E E))
(relation Noise (E E i64)) (relation Tick (i64)) (relation Seen (E i64))
(ruleset a) (ruleset b)
(rule ((Tick n) (Link n x y)) ((union x y) (Tick (+ n 1))) :ruleset a)
(rule ((Noise x x n)) ((Seen x n)) :ruleset b)
(Tick 0)
'''+''.join(f'(Link {i} (N {2*i}) (N {2*i+1}))\n' for i in range(32))+
        ''.join(f'(Noise (N {2*i}) (N {2*i+1}) {i})\n' for i in range(4096))+
        '(run-schedule (saturate (run a) (run b)))')
    core=core_for(source,build_dir);plan=from_core(core);initial=core.pop('initial')
    lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
    original=runtime.lower;reports={}
    for mode in ['serial','parallel','wrapped-generation']:
        def lower(*args,**kwargs):
            p=original(*args,**kwargs,parallel_indexes=mode!='serial')
            if mode=='wrapped-generation':
                target='bool reset=m.rounds==1||r[QUERY_DIRTY]==1;';assert p['shader'].count(target)==1
                p['shader']=p['shader'].replace(target,'if(m.rounds==1)qheads[Q_TOUCH]=0xffffffff;'+target)
            return p
        runtime.lower=lower
        try:reports[mode]=runtime.run_library(lib,build_dir,out/mode,16384,1024,region_program=plan,audit_numeric=True)
        finally:runtime.lower=original
        assert reports[mode]['complete']
    verify_repeat(reports['serial'],reports['parallel']);verify_repeat(reports['serial'],reports['wrapped-generation'])
    seen=rows(reports['parallel'],out/'parallel','Seen')
    assert len(seen)==32 and {r[1] for r in seen}==set(range(32))
    result=dict(initial_rows=len(initial),unions=32,
        contract='actual resident runtime; same complete state, per-Run fingerprints and observable consumer effects',
        correctness='union enables a repeated-variable match; stable radix links and wrapped-generation tags checked',
        timing_scope='one checked diagnostic per variant; not full Pass-3 speedup',
        seconds={k:dict(gpu=v['programs'][0]['gpu_seconds'],inference=v['programs'][0]['inference_seconds']) for k,v in reports.items()})
    (out/'index-results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2));return result


def check_lazy_contexts(build_dir,out):
    import run_generic_cells as runtime
    out.mkdir(parents=True,exist_ok=False);source=out/'context-reclamation.egg'
    source.write_text('''(datatype E (N i64))
(function V (E) i64 :merge (min old new)) (function F (E) i64 :merge (min old new))
(relation Noise (E)) (relation Bad (E)) (ruleset a) (ruleset b)
(rule ((Noise x) (= n (V x)) (= limit (F x)) (> n limit)) ((set (V x) (- n 1))) :ruleset a)
(rule ((Noise x) (= n (F x)) (< n 0)) ((Bad x)) :ruleset b)
'''+''.join(f'(Noise (N {i})) (set (F (N {i})) 2)\n' for i in range(2048))+
        ''.join(f'(set (V (N {i})) 64)\n' for i in range(64))+
        '(run-schedule (saturate (run a) (run b)))')
    core=core_for(source,build_dir);plan=from_core(core);initial=core.pop('initial')
    lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
    reports={};original=runtime.lower
    for lazy in [False,True]:
        def lower(*args,**kwargs):return original(*args,**kwargs,lazy_contexts=lazy,spine_cache=True)
        runtime.lower=lower
        try:reports[lazy]=runtime.run_library(lib,build_dir,out/str(lazy),8192,1024,region_program=plan,audit_numeric=True)
        finally:runtime.lower=original
        assert reports[lazy]['complete']
    a,b=[v['programs'][0]['instances'][0] for v in reports.values()]
    for field in ['meta','rows','parents','fact_parents','sets','watermarks','history']:
        assert a[field]==b[field],field
    assert b['region_statistics'][8]<a['region_statistics'][8]
    assert not rows(reports[True],out/'True','Bad')
    assert all(r[1]==2 for r in rows(reports[True],out/'True','V'))
    assert b['region_statistics'][8]>0, 'must exercise reclamation, not only its bypass'
    result=dict(contract='exact visible state, source progress and every Run fingerprint equal; maintenance counts may differ',
        rounds=len(a['history']),compactions={'eager':a['region_statistics'][8],'lazy':b['region_statistics'][8]},
        cache_capacity_words={'eager':a['region_statistics'][1],'lazy':b['region_statistics'][1]},
        seconds={str(k):dict(inference=v['programs'][0]['inference_seconds'],gpu=v['programs'][0]['gpu_seconds']) for k,v in reports.items()})
    (out/'lazy-results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2));return result


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--case',action='append');p.add_argument('--real-dir',type=Path);p.add_argument('--sort-comparison',action='store_true');p.add_argument('--pipeline-cache-program',type=Path);p.add_argument('--index-comparison',action='store_true');p.add_argument('--lazy-context-comparison',action='store_true');a=p.parse_args()
    if a.pipeline_cache_program:check_pipeline_cache(a.build.resolve(),a.output.resolve(),a.pipeline_cache_program.resolve())
    elif a.lazy_context_comparison:check_lazy_contexts(a.build.resolve(),a.output.resolve())
    elif a.index_comparison:check_indexes(a.build.resolve(),a.output.resolve())
    elif a.sort_comparison:check_sort(a.build.resolve(),a.output.resolve())
    else:check(a.build.resolve(),a.output.resolve(),a.case,a.real_dir)

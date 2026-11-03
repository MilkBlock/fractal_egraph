"""Ordered, bounded-storage constructor batches in the actual resident runtime."""
import argparse,json,sys,subprocess,copy
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
import run_generic_cells as runtime
from test_generic_cells import core_for
from verify_native_source import verify
from eggfuse.analyzer.regions import from_core

BASE='''(datatype E (N i64) (P E i64) (Q E) (R E i64))
(relation Seed (E i64)) (relation Gate (E E))
(function Count () i64 :merge (+ old new)) (set (Count) 0)
(ruleset go)
'''

def check(build,out,bench=False):
    out.mkdir(parents=True,exist_ok=False);N=32768 if bench else 513
    seeds=''.join(f'(Seed (N {i}) {i})\n' for i in range(N))
    cases={'independent':BASE+'(rule ((Seed x n)) ((P x n) (Q x)) :ruleset go)'+seeds+'(run go 1)'}
    if not bench:
        cases['duplicates']=BASE+'(rule ((Seed x n)) ((P x (% n 4)) (Q x)) :ruleset go)'+''.join(f'(Seed (N 0) {i})' for i in range(N))+'(run go 1)'
        cases['constructor-prediction']=BASE+'''
(rule ((Seed x n)) ((union (P x n) x)) :ruleset go)
(rule ((Seed x n)) ((P x n)) :ruleset go)
'''+seeds+'(run go 1)'
        cases['mixed-order']=BASE+'''
(rule ((Gate left right)) ((set (Count) 1)) :ruleset go)
(rule ((Seed x n)) ((P x n) (Q x)) :ruleset go)
(rule ((Gate left right)) ((union left right)) :ruleset go)
(rule ((Seed x n)) ((R x n)) :ruleset go)
(rule ((Gate left right)) ((set (Count) 10)) :ruleset go)
'''+seeds+'(Gate (N 0) (N 1)) (run go 1)'
    original=runtime.lower;result={}
    for name,text in cases.items():
        source=out/(name+'.egg');source.write_text(text);c=core_for(source,build);region=from_core(c);initial=c.pop('initial')
        lib=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=initial)])
        reports={}
        for parallel in [False,True]:
            runtime.lower=lambda *a,**kw:original(*a,**kw,parallel_constructors=parallel)
            try:reports[parallel]=runtime.run_library(lib,build,out/(name+'-'+str(parallel)),max(8192,N*6),max(8192,N*4),region_program=region,audit_numeric=not bench)
            finally:runtime.lower=original
            assert reports[parallel]['complete']
            if not bench:verify(source,build,out/(name+'-'+str(parallel))/'program-0/cell-0',out/(name+'-'+str(parallel)+'-native'))
        states=[r['programs'][0]['instances'][0] for r in reports.values()]
        for field in ('rows','parents','fact_parents','sets','watermarks','meta','history'):assert states[0][field]==states[1][field],(name,field)
        assert states[1]['batch_statistics'][32]>0,name
        if name=='constructor-prediction':assert states[1]['batch_statistics'][33]>0
        if name=='mixed-order':
            tid=next(i for i,t in enumerate(c['tables']) if t['name']=='Count')
            assert [r[4] for r in states[1]['rows'] if r[0]==tid]==[11]
        result[name]=dict(bindings=N,exact_state_and_run_history_equal=True,
            seconds={str(k):v['programs'][0]['inference_seconds'] for k,v in reports.items()},
            parallel_counters=states[1]['batch_statistics'][28:],
            dynamic_capacity=reports[True]['programs'][0]['attempts'][0]['write_capacity'],
            scope='real resident runtime; one paired diagnostic; not full Pass 3 speedup')
        print(name,result[name],flush=True)
    if not bench:
        # A completed neighbor must survive another arena's proposal growth.
        source=out/'independent.egg';c=core_for(source,build);region=from_core(c);initial=c.pop('initial')
        one=[];seen=False
        for row in initial:
            if row['table']=='Seed':
                if seen:continue
                seen=True
            one.append(copy.deepcopy(row))
        lib=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=one),dict(unit=1,program=0,initial=initial)])
        runs=[runtime.run_library(lib,build,out/('neighbors-'+name),8192,8192,region_program=region,audit_numeric=True) for name in ['first','repeat']]
        from eggfuse.engine.regions import verify_repeat
        verify_repeat(*runs)
        tid=next(i for i,t in enumerate(c['tables']) if t['name']=='P')
        assert [sum(row[0]==tid for row in state['rows']) for state in runs[0]['programs'][0]['instances']]==[1,N]
        verify(source,build,out/'neighbors-first/program-0/cell-1',out/'neighbors-native')
        assert runs[0]['programs'][0]['attempts'][0]['write_growths']>0
        result['neighbor-growth']=dict(independent_arenas=2,repeatable=True,exact_large_arena_native=True,no_cross_arena_rows=True)
        # A failed vector proposal must fall back before publishing any of it;
        # the ordered path retains the P write preceding the overflowing add.
        source=out/'overflow.egg'
        source.write_text(BASE+'(rule ((Seed x n)) ((P x n) (R x (+ n 1))) :ruleset go)'+
            ''.join(f'(Seed (N 0) {i})' for i in range(512))+'(Seed (N 0) 9223372036854775807) (run go 1)')
        c=core_for(source,build);region=from_core(c);initial=c.pop('initial')
        lib=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=initial)])
        states=[]
        for suffix in ['first','repeat']:
            r=runtime.run_library(lib,build,out/('overflow-'+suffix),8192,8192,region_program=region,audit_numeric=True)
            state=r['programs'][0]['instances'][0];assert state['meta'][4]==5;states.append(state)
        assert states[0]==states[1]
        tid=next(i for i,t in enumerate(c['tables']) if t['name']=='P')
        assert any(row[0]==tid and row[5]==9223372036854775807 for row in states[0]['rows'])
        native=subprocess.run([str(build/'frontend'),str(source),'--native-full','--export-to',str(out/'overflow-native')],text=True,capture_output=True)
        assert native.returncode!=0
        result['overflow']=dict(source_error_preserved=True,preceding_P_effect_retained=True,private_proposals_not_published=True)
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--bench',action='store_true');a=p.parse_args();check(a.build.resolve(),a.output.resolve(),a.bench)

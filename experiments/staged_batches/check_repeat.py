"""Native differential checks for actual resident private Repeat completion."""
import argparse,json,sys,copy
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
from test_generic_cells import core_for
from verify_native_source import verify
from run_generic_cells import run_library
from eggfuse.analyzer.regions import from_core
from eggfuse.engine.regions import verify_repeat

BASE='''(datatype E (N i64))
(constructor P (E) E) (constructor Q (E) E) (constructor R (E) E)
(ruleset a) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((P x)) ((Q x)) :ruleset a)
(rule ((Q x)) ((R x)) :ruleset b)
(N 0) (N 1)
'''

def check(build,out):
    out.mkdir(parents=True,exist_ok=False)
    cases={
      'zero':BASE+'(run-schedule (repeat 0 (run a) (run b)))',
      'one':BASE+'(run-schedule (repeat 1 (run a) (run b)))',
      'two':BASE+'(run-schedule (repeat 2 (run a) (run b)))',
      'nested':BASE+'(run-schedule (repeat 5 (repeat 1 (run a)) (saturate (run b))))',
      'tree':'''(datatype E (N i64) (Pair E E)) (constructor P (E) E)
(ruleset seed) (ruleset propagate)
(rule ((= x (N n))) ((P x)) :ruleset seed)
(rule ((= z (Pair x y)) (P x) (P y)) ((P z)) :ruleset propagate)
(Pair (N 0) (Pair (N 1) (N 2)))
(run-schedule (run seed) (repeat 1 (run propagate)))''',
      'escaped-result':BASE+'''(function Chosen () E :merge new) (ruleset export)
(rule ((= p (P x))) ((set (Chosen) p)) :ruleset export)
(run-schedule (repeat 2 (run a) (run b)) (run export))''',
      'subsume-boundary':BASE+'''(ruleset hide)
(rule ((= x (N 0))) ((subsume (P x))) :ruleset hide)
(run-schedule (repeat 1 (run a) (run b)) (run hide) (repeat 2 (run a) (run b)))''',
      'union-boundary':BASE+'''(ruleset merge)
(rule ((= left (N 0)) (= right (N 1))) ((union left right)) :ruleset merge)
(run-schedule (repeat 1 (run a) (run b)) (run merge) (repeat 2 (run a) (run b)))''',
    }
    results={}
    for name,text in cases.items():
        source=out/(name+'.egg');source.write_text(text);core=core_for(source,build);plan=from_core(core);initial=core.pop('initial')
        lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
        a=run_library(lib,build,out/(name+'-first'),capacity=256,match_capacity=256,region_program=plan,audit_numeric=True)
        b=run_library(lib,build,out/(name+'-repeat'),capacity=256,match_capacity=256,region_program=plan,audit_numeric=True)
        assert a['complete'] and b['complete'],name
        repeat=verify_repeat(a,b)
        native=verify(source,build,out/(name+'-first/program-0/cell-0'),out/(name+'-native'))
        attempt=a['programs'][0]['attempts'][0]
        if name!='escaped-result':assert attempt['private_schedule_regions']>0,(name,attempt)
        results[name]=dict(repeat=repeat,native=native,private_regions=attempt['private_schedule_regions'],private_runs=attempt['private_schedule_runs'],private_rows=attempt['private_schedule_rows'])
        print(name,results[name]['private_runs'],native,flush=True)
    (out/'results.json').write_text(json.dumps(results,indent=2)+'\n')

def bench(build,out):
    import run_generic_cells as runtime
    out.mkdir(parents=True,exist_ok=False);source=out/'finite-chain.egg'
    source.write_text('(datatype E (N i64)) (ruleset step)\n'+
        ''.join(f'(constructor P{i} (E) E)\n' for i in range(28))+
        '(rule ((= x (N n))) ((P0 x)) :ruleset step)\n'+
        ''.join(f'(rule ((P{i-1} x)) ((P{i} x)) :ruleset step)\n' for i in range(1,28))+
        ''.join(f'(N {i})\n' for i in range(256))+'(run-schedule (repeat 28 (run step)))')
    core=core_for(source,build);plan=from_core(core);initial=core.pop('initial')
    lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
    reports={};original=runtime.lower
    for fused in [False,True]:
        def lower(*a,**kw):return original(*a,**kw,scheduled_regions=fused)
        runtime.lower=lower
        try:reports[fused]=runtime.run_library(lib,build,out/str(fused),8192,8192,region_program=plan)
        finally:runtime.lower=original
        assert reports[fused]['complete']
        verify(source,build,out/str(fused)/'program-0/cell-0',out/('native-'+str(fused)))
    result=dict(scope='actual resident engine, finite Repeat 28; single measurement per mode; not Raytrace speedup',
        inputs=256,stages=28,seconds={str(k):v['programs'][0]['inference_seconds'] for k,v in reports.items()},
        rows={str(k):len(v['programs'][0]['instances'][0]['rows']) for k,v in reports.items()},
        private={str(k):{f:v['programs'][0]['attempts'][0][f] for f in ['private_schedule_regions','private_schedule_runs','private_schedule_rows','private_schedule_forwarded_reads']} for k,v in reports.items()})
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--bench',action='store_true');a=p.parse_args();(bench if a.bench else check)(a.build.resolve(),a.output.resolve())

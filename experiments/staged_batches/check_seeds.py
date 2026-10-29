"""Delta job compaction preserves fresh, old, grounded and scalar query cases."""
import argparse,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
import run_generic_cells as runtime
from test_generic_cells import core_for
from verify_native_source import verify
from eggfuse.analyzer.regions import from_core


def check(build,out):
    out.mkdir(parents=True,exist_ok=False)
    cases={'grounded-and-old':'''(datatype E (N i64) (Pair E E))
(function F (i64) E :merge new) (relation Seen (E)) (relation Token (E E))
(ruleset observe) (ruleset edit) (ruleset init)
(rule () ((N 42)) :ruleset init)
(rule ((= pair (Pair x y)) (= x (F 0))) ((Seen y)) :ruleset observe)
(rule ((= x (F 0)) (= z (N 42))) ((Token x z)) :ruleset observe)
(rule ((= x (N 1))) ((set (F 0) x)) :ruleset edit)
(Pair (N 0) (N 2)) (Pair (N 1) (N 3)) (set (F 0) (N 0))
(run-schedule (run observe) (run init) (run edit) (run observe))''',
      'scalar-query':'''(sort E) (constructor N (i64) E) (sort S (Set E)) (constructor Box (S) E)
(relation Seen (E)) (ruleset calc)
(rule ((= x (Box s)) (= t (set-union s s))) ((Seen x)) :ruleset calc)
(Box (set-insert (set-empty) (N 0))) (run calc 1)'''}
    results={};orig=runtime.lower
    for name,text in cases.items():
        source=out/(name+'.egg');source.write_text(text);c=core_for(source,build);plan=from_core(c);initial=c.pop('initial')
        lib=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=initial)])
        reports={}
        for compact in [False,True]:
            runtime.lower=lambda *a,**kw:orig(*a,**kw,compact_seeds=compact)
            try:reports[compact]=runtime.run_library(lib,build,out/(name+'-'+str(compact)),4096,4096,region_program=plan,audit_numeric=True)
            finally:runtime.lower=orig
            assert reports[compact]['complete']
            verify(source,build,out/(name+'-'+str(compact))/'program-0/cell-0',out/(name+'-'+str(compact)+'-native'))
        a,b=[r['programs'][0]['instances'][0] for r in reports.values()]
        for f in ('rows','parents','fact_parents','sets','watermarks','meta','history'):assert a[f]==b[f],(name,f)
        results[name]=dict(exact_native_and_reference=True,seed_counters=b['batch_statistics'][36:39])
        if name=='grounded-and-old':assert b['batch_statistics'][38]>0
    (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');print(json.dumps(results,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();check(a.build.resolve(),a.output.resolve())

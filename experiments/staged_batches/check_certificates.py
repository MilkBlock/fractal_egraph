"""Exact row-revision tickets survive unrelated edits, not same-epoch changes."""
import json,sys,argparse
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
import run_generic_cells as runtime
from test_generic_cells import core_for
from verify_native_source import verify
from eggfuse.analyzer.regions import from_core

BASE='''(datatype E (N i64) (Mid E) (Done E) (Other E))
(relation Seed (E)) (relation Link (E E))
(ruleset produce) (ruleset finish)
(rule ((Seed x)) ((Mid x)) :ruleset produce)
(rule ((Link left right)) ((union left right)) :ruleset produce)
(rule ((= result (Mid x))) ((Done x)) :ruleset finish)
'''

def check(build,out):
    out.mkdir(parents=True,exist_ok=False);reports={}
    inputs={'unrelated':'(N 0) (Seed (N 30)) (Link (N 98) (N 99))',
            'same-epoch-edit':'(N 0) (Seed (N 30)) (Link (N 30) (N 0))'}
    original=runtime.lower
    for case,seed in inputs.items():
        source=out/(case+'.egg');source.write_text(BASE+seed+'(run-schedule (saturate (run produce) (run finish)))')
        c=core_for(source,build);plan=from_core(c);initial=c.pop('initial')
        lib=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=initial)])
        states={};timings={}
        for local in [False,True]:
            runtime.lower=lambda *a,**kw:original(*a,**kw,local_certificates=local)
            try:r=runtime.run_library(lib,build,out/(case+'-'+str(local)),256,256,region_program=plan,audit_numeric=True)
            finally:runtime.lower=original
            assert r['complete'];s=r['programs'][0]['instances'][0];states[local]=s
            verify(source,build,out/(case+'-'+str(local))/'program-0/cell-0',out/(case+'-'+str(local)+'-native'))
            timings[str(local)]=r['programs'][0]['inference_seconds']
        a,b=states.values()
        for field in ('rows','parents','fact_parents','sets','watermarks'):assert a[field]==b[field],(case,field)
        if case=='unrelated':assert b['batch_statistics'][12]>a['batch_statistics'][12]
        else:assert a['batch_statistics'][12]==b['batch_statistics'][12], 'changed witness must be rechecked'
        reports[case]=dict(native_facts_equal=True,
            shortcuts={str(k):v['batch_statistics'][12] for k,v in states.items()},
            bindings={str(k):sum(h[2] for h in v['history']) for k,v in states.items()},seconds=timings)
    (out/'results.json').write_text(json.dumps(reports,indent=2)+'\n');print(json.dumps(reports,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();check(a.build.resolve(),a.output.resolve())

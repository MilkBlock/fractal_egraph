"""One changing key must not force keyed re-probes of every stable anchor."""
import argparse,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
import run_generic_cells as runtime
from test_generic_cells import core_for
from verify_native_source import verify
from eggfuse.analyzer.regions import from_core


def check(build,out):
    out.mkdir(parents=True,exist_ok=False);source=out/'single-changing-port.egg'
    source.write_text('''(datatype E (N i64)) (relation Anchor (E)) (relation Seen (E))
(function V (E) i64 :merge (min old new)) (function Limit (E) i64 :merge (min old new))
(ruleset tick) (ruleset observe)
(rule ((= x (N 0)) (= n (V x)) (> n 0)) ((set (V x) (- n 1))) :ruleset tick)
(rule ((Anchor x) (= n (V x)) (= lim (Limit x)) (<= n lim)) ((Seen x)) :ruleset observe)
'''+''.join(f'(Anchor (N {i})) (set (V (N {i})) 64) (set (Limit (N {i})) 0)\n' for i in range(2048))+
        '(run-schedule (saturate (run tick) (run observe)))')
    c=core_for(source,build);plan=from_core(c);initial=c.pop('initial')
    lib=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=initial)])
    reports={};orig=runtime.lower
    for cached in [False,True]:
        runtime.lower=lambda *a,**kw:orig(*a,**kw,spine_cache=cached)
        try:reports[cached]=runtime.run_library(lib,build,out/str(cached),16384,8192,region_program=plan,audit_numeric=True)
        finally:runtime.lower=orig
        assert reports[cached]['complete']
        verify(source,build,out/str(cached)/'program-0/cell-0',out/('native-'+str(cached)))
    a,b=[r['programs'][0]['instances'][0] for r in reports.values()]
    for f in ('rows','parents','fact_parents','sets','watermarks','meta','history'):assert a[f]==b[f],f
    assert b['region_statistics'][11]>120000,b['region_statistics']
    result=dict(exact_native_and_uncached_state=True,stable_tuple_reuses=b['region_statistics'][11],
        keyed_reads=b['region_statistics'][6],rounds=len(b['history']),
        seconds={str(k):r['programs'][0]['inference_seconds'] for k,r in reports.items()},
        scope='single diagnostic; not Pass 3 speedup')
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();check(a.build.resolve(),a.output.resolve())

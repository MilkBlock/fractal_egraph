"""Constant-time bucket updates with source-order binding witness coordinates."""
import argparse,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
import run_generic_cells as runtime
from test_generic_cells import core_for
from verify_native_source import verify
from eggfuse.analyzer.regions import from_core


def check(build,out,n):
    out.mkdir(parents=True,exist_ok=False);source=out/'shared-value-bucket.egg'
    source.write_text('''(datatype E (N i64) (Tag i64))
(function F (E) i64 :merge (min old new))
(relation Update (E i64)) (relation Found (E))
(ruleset change) (ruleset observe)
(rule ((Update x val)) ((set (F x) val)) :ruleset change)
(rule ((= tag (Tag val)) (= val (F x))) ((Found x)) :ruleset observe)
(Tag 0)
'''+''.join(f'(set (F (N {i})) {i%2})\n'+(f'(Update (N {i}) 0)\n' if i%2 else '') for i in range(n))+
        '(run-schedule (run change) (run observe))')
    c=core_for(source,build);region=from_core(c);initial=c.pop('initial')
    lib=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=initial)])
    reports={};original=runtime.lower
    for ordered in [True,False]:
        runtime.lower=lambda *a,**kw:original(*a,**kw,ordered_query_patch=ordered)
        try:reports[ordered]=runtime.run_library(lib,build,out/str(ordered),max(4096,4*n),max(1024,2*n),region_program=region,audit_numeric=True)
        finally:runtime.lower=original
        assert reports[ordered]['complete']
        verify(source,build,out/str(ordered)/'program-0/cell-0',out/('native-'+str(ordered)))
    a,b=[r['programs'][0]['instances'][0] for r in reports.values()]
    for f in ('rows','parents','fact_parents','sets','watermarks','meta','history'):assert a[f]==b[f],f
    tid=next(i for i,t in enumerate(c['tables']) if t['name']=='Found');assert sum(row[0]==tid for row in b['rows'])==n
    result=dict(inputs=n,updates=n//2,exact_native_facts_and_original_order=True,
        inference_seconds={str(k):r['programs'][0]['inference_seconds'] for k,r in reports.items()},
        scope='one paired resident-runtime measurement; not full Pass 3 speedup',
        contract='O(1) unlink/prepend; actual loop-row coordinates restore source binding order before effects')
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--rows',type=int,default=4096);a=p.parse_args();check(a.build.resolve(),a.output.resolve(),a.rows)

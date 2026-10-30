"""A Cell union rewrite consumes the constructor fact it already matched."""
import argparse,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
import run_generic_cells as runtime
from test_generic_cells import core_for
from verify_native_source import verify
from eggfuse.analyzer.regions import from_core


def check(build,out,n):
    out.mkdir(parents=True,exist_ok=False);source=out/'witnessed-rewrite.egg'
    source.write_text('''(datatype E (N i64) (TCPair E i64) (Smaller E E)) (ruleset steps)
(rule ((= lhs (Smaller (TCPair left cx) (TCPair right cy))) (<= cx cy))
 ((union lhs (TCPair left cx))) :ruleset steps)
'''+''.join(f'(Smaller (TCPair (N {2*i}) 0) (TCPair (N {2*i+1}) 1))\n' for i in range(n))+'(run-schedule (saturate (run steps)))')
    c=core_for(source,build);region=from_core(c);initial=c.pop('initial')
    lib=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=initial)])
    reports={};orig=runtime.lower
    for reuse in [False,True]:
        runtime.lower=lambda *a,**kw:orig(*a,**kw,reuse_constructor_witnesses=reuse,materialize_single_steps=True)
        try:reports[reuse]=runtime.run_library(lib,build,out/str(reuse),max(4096,8*n),max(1024,4*n),region_program=region,audit_numeric=False)
        finally:runtime.lower=orig
        assert reports[reuse]['complete']
        verify(source,build,out/str(reuse)/'program-0/cell-0',out/('native-'+str(reuse)))
    a,b=[r['programs'][0]['instances'][0] for r in reports.values()]
    for field in ('rows','parents','fact_parents','sets','watermarks','meta','history'):assert a[field]==b[field],field
    assert b['batch_statistics'][41]>0
    result=dict(seeds=n,exact_reference_and_native=True,witness_reads=b['batch_statistics'][40],witness_sets=b['batch_statistics'][41],
        seconds={str(k):r['programs'][0]['inference_seconds'] for k,r in reports.items()},scope='one paired resident microbenchmark; not full Pass 3 speedup')
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--rows',type=int,default=8192);a=p.parse_args();check(a.build.resolve(),a.output.resolve(),a.rows)

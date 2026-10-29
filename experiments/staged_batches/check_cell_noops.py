"""Exact no-op proofs for single-step Cell seeds; no shortcut for descendants."""
import argparse,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
import run_generic_cells as runtime
from test_generic_cells import core_for
from verify_native_source import verify
from eggfuse.analyzer.regions import from_core


def check(build,out,n,hierarchical=False):
    out.mkdir(parents=True,exist_ok=False);source=out/'already-better.egg'
    source.write_text('''(datatype E (N i64) (Pair E E))
(function Size (E) i64 :merge (min old new)) (ruleset calc)
(rule ((= root (Pair x y)) (= sx (Size x)) (= sy (Size y)))
 ((set (Size root) (+ 1 (+ sx sy)))) :ruleset calc)
'''+''.join(f'(set (Size (N {2*i})) 20) (set (Size (N {2*i+1})) 30) (set (Size (Pair (N {2*i}) (N {2*i+1}))) 0)\n' for i in range(n))+
        '(run-schedule (saturate (run calc)))')
    c=core_for(source,build);plan=from_core(c);initial=c.pop('initial')
    lib=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=initial)])
    original=runtime.lower;reports={}
    for enabled in [False,True]:
        runtime.lower=(lambda *a,**kw:original(*a,**kw,hierarchical_indexes=enabled,cell_noops=True)) if hierarchical else (lambda *a,**kw:original(*a,**kw,cell_noops=enabled))
        try:reports[enabled]=runtime.run_library(lib,build,out/str(enabled),max(4096,8*n),max(1024,2*n),region_program=plan,audit_numeric=True)
        finally:runtime.lower=original
        assert reports[enabled]['complete']
        verify(source,build,out/str(enabled)/'program-0/cell-0',out/('native-'+str(enabled)))
    a,b=[r['programs'][0]['instances'][0] for r in reports.values()]
    for f in ('rows','parents','fact_parents','sets','watermarks','meta','history'):assert a[f]==b[f],f
    assert a['batch_statistics'][34]==(n if hierarchical else 0) and b['batch_statistics'][34]==n
    if hierarchical:
        p=json.load(open(out/'True/program-0/cell-0/program.json'))
        pairs=sum(len(p['index_columns'][row['table']]) for row in initial)
        assert pairs>65536,(pairs,n)
    result=dict(mode='hierarchical-index-scan' if hierarchical else 'cell-noops',seeds=n,skipped_cell_seeds=b['batch_statistics'][34],checks=b['batch_statistics'][35],
        exact_native_and_reference_state=True,seconds={str(k):r['programs'][0]['inference_seconds'] for k,r in reports.items()},scope='one paired resident microbenchmark; not full Pass 3 speedup')
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--rows',type=int,default=8192);p.add_argument('--hierarchical',action='store_true');a=p.parse_args();check(a.build.resolve(),a.output.resolve(),a.rows,a.hierarchical)

"""Restore resident output/timestamp index patching, preserving source order."""
import argparse
import json
from pathlib import Path
import sys
ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
import run_generic_cells as runtime
from test_generic_cells import core_for
from eggfuse.analyzer.regions import from_core


def check(build,out):
    out.mkdir(parents=True,exist_ok=False)
    source=out/'changing-values.egg'
    source.write_text('''(datatype E (N i64))
(function V (E) i64 :merge (min old new)) (relation Seen (E i64))
(relation Noise (i64)) (relation Bad (i64)) (ruleset a) (ruleset b)
(rule ((= n (V x)) (> n 0)) ((set (V x) (- n 1))) :ruleset a)
(rule ((= n (V x)) (= n 7)) ((Seen x n)) :ruleset b)
(rule ((Noise n) (< n 0)) ((Bad n)) :ruleset b)
'''+''.join(f'(set (V (N {i})) 32)\n' for i in range(64))+
        ''.join(f'(Noise {i})\n' for i in range(16384))+
        '(run-schedule (saturate (run a) (run b)))')
    core=core_for(source,build);plan=from_core(core);initial=core.pop('initial')
    lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
    original=runtime.lower;reports={}
    for enabled in [False,True]:
        def lower(*args,**kwargs):return original(*args,**kwargs,patch_queries=enabled)
        runtime.lower=lower
        try:reports[enabled]=runtime.run_library(lib,build,out/str(enabled),32768,2048,region_program=plan,audit_numeric=True)
        finally:runtime.lower=original
        assert reports[enabled]['complete']
    a,b=[r['programs'][0]['instances'][0] for r in reports.values()]
    for field in a:
        if field=='state':
            assert a[field][:5]+a[field][7:]==b[field][:5]+b[field][7:]
        else:assert a[field]==b[field],field
    assert a['state'][5]>20 and b['state'][5]==1
    result=dict(scope='actual resident Metal engine, changing function values and an output-key consumer',
        initial_rows=len(initial),final_rows=len(a['rows']),runs=len(a['history']),
        full_rows_ids_visibility_and_history_equal=True,
        full_query_index_resets={'before':a['state'][5],'after':b['state'][5]},
        diagnostic_seconds={str(k):dict(inference=v['programs'][0]['inference_seconds'],gpu=v['programs'][0]['gpu_seconds']) for k,v in reports.items()},
        credit='query index maintenance; not Cell proof or complete Pass-3 speedup')
    (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2));return result


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();check(a.build.resolve(),a.output.resolve())

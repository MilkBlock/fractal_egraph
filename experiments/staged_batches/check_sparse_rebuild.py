"""A/B resident scratch-index reset; allocation size does not imply live work."""
import argparse
import json
from pathlib import Path
import sys

ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
import run_generic_cells as runtime
from test_generic_cells import core_for
from eggfuse.analyzer.regions import from_core


def check(build,out,capacity):
    out.mkdir(parents=True,exist_ok=False)
    source=out/'incremental-unions.egg'
    source.write_text('''(datatype E (N i64)) (relation Link (i64 E E))
(relation Tick (i64)) (ruleset s)
(rule ((Tick n) (Link n x y)) ((union x y) (Tick (+ n 1))) :ruleset s)
(Tick 0)
'''+''.join(f'(Link {i} (N {2*i}) (N {2*i+1}))\n' for i in range(32))+'(run-schedule (saturate (run s)))')
    core=core_for(source,build);plan=from_core(core);initial=core.pop('initial')
    lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
    original=runtime.lower;reports={}
    for mode in ['clear-capacity','clear-live','generation-wrap']:
        def lower(*args,**kwargs):
            p=original(*args,**kwargs)
            needle='bool sparseGroups=RC_ENABLED&&!QUERY_PATCH;'
            assert p['shader'].count(needle)==1
            if mode=='clear-capacity':p['shader']=p['shader'].replace(needle,'bool sparseGroups=false;')
            if mode=='generation-wrap':
                # First query -> UINT_MAX, first rebuild -> 0 -> reset to 1.
                target='bool reset=m.rounds==1||r[QUERY_DIRTY]==1;'
                assert p['shader'].count(target)==1
                p['shader']=p['shader'].replace(target,'if(m.rounds==1)qheads[Q_TOUCH]=0xfffffffe;'+target)
            return p
        runtime.lower=lower
        try:reports[mode]=runtime.run_library(lib,build,out/mode,capacity,1024,region_program=plan,audit_numeric=True)
        finally:runtime.lower=original
        assert reports[mode]['complete']
    states=[r['programs'][0]['instances'][0] for r in reports.values()]
    assert states[0]==states[1]==states[2], 'scratch-index reset changed semantic state or execution counts'
    a=states[0];assert a['meta'][7]==32
    result=dict(scope='actual Metal resident runtime; 32 incremental unions; allocator-capacity stress, not full Pass 3',
                capacity=capacity,initial_rows=len(initial),final_rows=len(a['rows']),
                rounds=len(a['history']),unions=a['meta'][7],
                full_states_and_rounds_equal=True,generation_wrap_checked=True,
                diagnostic_seconds={mode:dict(gpu=report['programs'][0]['gpu_seconds'],
                    inference=report['programs'][0]['inference_seconds']) for mode,report in reports.items()},
                credit='executor scratch-index maintenance; not Cell proof or matching coverage')
    (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2));return result


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--capacity',type=int,default=1048576)
    a=p.parse_args();check(a.build.resolve(),a.output.resolve(),a.capacity)

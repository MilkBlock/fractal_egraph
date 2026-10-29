"""Physical index-maintenance regression; no Cell credit or timing claim."""
import argparse,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
import run_generic_cells as runtime
from test_generic_cells import core_for
from eggfuse.analyzer.regions import from_core


def check(build,out):
    out.mkdir(parents=True,exist_ok=False)
    source=out/'conflicts.egg'
    source.write_text('''(datatype E (N i64) (C E E))
(function V (E) E :merge (C old new))
(relation Link (E E)) (ruleset s)
(rule ((Link x y)) ((union x y)) :ruleset s)
'''+''.join(f'(Link (N {2*i}) (N {2*i+1})) (set (V (N {2*i})) (N {1000+2*i})) (set (V (N {2*i+1})) (N {1001+2*i}))\n' for i in range(64))+'(run-schedule (saturate (run s)))')
    core=core_for(source,build);plan=from_core(core);initial=core.pop('initial')
    library=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
    original=runtime.lower;reports={}
    for mode in ['invalidate-all','retire-exact']:
        def lower(*args,**kwargs):
            p=original(*args,**kwargs)
            if mode=='invalidate-all':
                needle='if(RC_ENABLED&&r[INDEX_OFFSET]==long(m.unions)+1)'
                assert p['shader'].count(needle)==1
                p['shader']=p['shader'].replace(needle,'if(false)').replace('else if(!RC_ENABLED)r[INDEX_OFFSET]=0;','else r[INDEX_OFFSET]=0;')
            return p
        runtime.lower=lower
        try:reports[mode]=runtime.run_library(library,build,out/mode,2048,1024,region_program=plan,audit_numeric=True)
        finally:runtime.lower=original
        assert reports[mode]['complete']
    a,b=[reports[k]['programs'][0]['instances'][0] for k in ['invalidate-all','retire-exact']]
    for field in ['rows','parents','fact_parents','sets','watermarks','meta','history','batch_statistics','region_statistics']:
        assert a[field]==b[field],field
    assert a['row_index_rebuilds']>10*b['row_index_rebuilds']
    result=dict(scope='actual Metal runtime; index invalidation only; same source and Cell recipes',
        conflicting_keys=64,full_states_and_rounds_equal=True,
        before_index_rebuilds=a['row_index_rebuilds'],after_index_rebuilds=b['row_index_rebuilds'],
        timing_claim=False,cell_credit=False)
    (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2));return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();check(a.build.resolve(),a.output.resolve())

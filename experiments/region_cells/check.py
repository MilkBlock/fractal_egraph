"""Generic region programs versus unoptimized GPU and independent native source."""
import argparse,copy,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT));sys.path.insert(0,str(ROOT/'experiments/device_backend'))
from test_generic_cells import core_for,CASES
from run_generic_cells import run_library
from verify_native_source import verify
from eggfuse.analyzer.regions import from_core,verify as verify_plan
from eggfuse.ir.recipe import build
from eggfuse.compiler.targets.metal.generic_cell import lower

BASE='''(datatype E (N i64) (Pair E E))
(function Cost (E) i64 :merge (min old new))
(relation A (E i64 i64)) (relation B (E i64 i64)) (relation Late ())
(ruleset first) (ruleset second) (ruleset edit)
(rule ((= r (Pair x y)) (= a (Cost x)) (= b (Cost y))) ((A r a b)) :ruleset first)
(rule ((= r (Pair x y)) (= a (Cost x)) (= b (Cost y))) ((B r a b)) :ruleset second)
'''
EXTRA={
'late-input':BASE+'''(rule ((Late)) ((set (Cost (N 1)) 7)) :ruleset edit)
(Pair (N 0) (N 1)) (set (Cost (N 0)) 2) (Late)
(run first 1) (run edit 1) (run second 1) (run first 1)
(check (A (Pair (N 0) (N 1)) 2 7))''',
'union-new-wiring':BASE+'''(rule ((Late)) ((union (N 1) (N 2))) :ruleset edit)
(Pair (N 0) (N 1)) (set (Cost (N 0)) 2) (set (Cost (N 2)) 7) (Late)
(run first 1) (run edit 1) (run second 1) (run first 1)
(check (A (Pair (N 0) (N 1)) 2 7))''',
'subsume-retirement':BASE+'''(rule ((Late)) ((subsume (Pair (N 0) (N 1)))) :ruleset edit)
(Pair (N 0) (N 1)) (set (Cost (N 0)) 2) (set (Cost (N 1)) 7) (Late)
(run first 1) (run edit 1) (run second 1)
''',
'correlated-alternatives':BASE+'''(Pair (N 0) (N 1)) (Pair (N 2) (N 3))
(set (Cost (N 0)) 2) (set (Cost (N 1)) 7)
(set (Cost (N 2)) 11) (set (Cost (N 3)) 13)
(run-schedule (repeat 2 (run first) (run second)))
(check (A (Pair (N 0) (N 1)) 2 7)) (check (B (Pair (N 2) (N 3)) 11 13))''',
}


def check(build_path,out,real_dir=None):
    out.mkdir(parents=True,exist_ok=False);results={}
    cases={**CASES,**EXTRA,'smoke':(ROOT/'experiments/region_cells/smoke.egg').read_text()}
    if real_dir is not None:
        for name in ['finite','late-union','context-to-terms','renamed']:
            cases['real-'+name]=(real_dir/(name+'.egg')).read_text()
    # Many actual anchor tuples force packed-arena growth; no row×plan reservation.
    cases['growth']=BASE+''.join(f'(Pair (N {2*i}) (N {2*i+1})) (set (Cost (N {2*i})) 2) (set (Cost (N {2*i+1})) 7)\n' for i in range(80))+'(run first 1) (run second 1)'
    for name,source in cases.items():
        file=out/(name+'.egg');file.write_text(source)
        core=core_for(file,build_path);plan=from_core(core)
        verify_plan(plan,build(core))
        if name=='smoke':
            bad=copy.deepcopy(plan);bad['nodes'][0]['atom']['args'][0]={'literal':999,'sort':core['tables'][0]['schema'][0]}
            try:verify_plan(bad,build(core))
            except ValueError:pass
            else:raise AssertionError('tampered wiring accepted')
        initial=core.pop('initial');lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
        lanes={}
        for mode in ['plain','regions']:
            folder=out/(name+'-'+mode)
            result=run_library(lib,build_path,folder,capacity=1024,match_capacity=4096,region_program=plan if mode=='regions' else None)
            assert result['complete'],(name,mode,result['rejected'])
            instance=result['programs'][0]['instances'][0]
            native=verify(file,build_path,folder/'program-0/cell-0',folder/'native')
            lanes[mode]=dict(check=native,history=instance['history'],stats=instance['region_statistics'],
                             growths=sum(a.get('region_cache_growths',0) for a in result['programs'][0]['attempts']))
        assert lanes['plain']['history']==lanes['regions']['history'],(name,'original Run decisions or matches changed')
        if name in ['late-input','union-new-wiring','subsume-retirement']:
            assert lanes['regions']['stats'][7]>0,(name,'no invalidation exercised')
        if name in ['correlated-alternatives','growth']:
            assert lanes['regions']['stats'][2]>0,(name,'no shared context reuse')
        if name=='growth':assert lanes['regions']['growths']>0
        results[name]=dict(nodes=len(plan['nodes']),rules=len(plan['rules']),regions=len(plan['regions']),
                           check=lanes['regions']['check'],stats=lanes['regions']['stats'],growths=lanes['regions']['growths'],same_original_runs_and_matches=True)
        (out/'results.json').write_text(json.dumps(results,indent=2));print(name,'passed',flush=True)
    return results

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--real-dir',type=Path);a=p.parse_args();check(a.build.resolve(),a.output.resolve(),a.real_dir)

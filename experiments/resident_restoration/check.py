"""Restore resident general backend capabilities; use actual engine and native oracle."""
import argparse,copy,json,sys
from pathlib import Path
from unittest.mock import patch
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
from test_generic_cells import core_for
from eggfuse.analyzer.regions import from_core
from eggfuse.engine.regions import execution_summary,verify_repeat
from verify_native_source import verify
import run_generic_cells as runtime

OPTIONS=('resume_merge_index','typed_seed_dispatch','direct_ordinary_rhs')

def run(core,plan,build,folder,on=True,audit=True,capacity=None,match_capacity=None,**options):
    lower=runtime.lower
    settings={k:on for k in OPTIONS};settings.update(options)
    with patch.object(runtime,'lower',lambda *a,**kw:lower(*a,**kw,**settings)):
        c=copy.deepcopy(core);initial=c.pop('initial')
        library=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=initial)])
        result=runtime.run_library(library,build,folder,capacity or max(8192,len(initial)*8),match_capacity or max(4096,len(initial)*4),region_program=plan,audit_numeric=audit)
    if not result['complete']:raise ValueError(result['rejected'])
    return result


def merge_source(n):
    return '''(datatype E (N i64) (Pair E E) (Wrap E))
(function F (E) E :merge (Pair old new)) (relation Join (E E)) (ruleset s)
(rule ((Join a b)) ((union a b)) :ruleset s)
'''+''.join(f'(set (F (N {2*i})) (N {100000+i}))\n(set (F (N {2*i+1})) (N {200000+i}))\n(Wrap (N {2*i}))\n(Wrap (N {2*i+1}))\n(Join (N {2*i}) (N {2*i+1}))\n' for i in range(n))+'(run-schedule (saturate s))\n'

CASES={
 'merge-resume':merge_source(129),
 'ordinary-finite':'''(datatype E (N i64) (Box E) (Pair E E)) (relation Seed (E))
(function Pick (E) E :merge (Pair old new)) (ruleset s)
(rule ((Seed x)) ((let b (Box x)) (set (Pick (N 0)) b)) :ruleset s)
'''+''.join(f'(Seed (N {i}))\n' for i in range(257))+'(run s 1)',
}
# Runtime fact commands after the first Run are rejected by the typed backend;
# express late inputs as scheduled producer rules instead.
CASES['typed-delta-and-grounded']='''(datatype E (N i64)) (relation A (E)) (relation B (E))
(relation C (E)) (relation UnitFact ()) (relation Out (E)) (ruleset s) (ruleset late)
(rule () ((UnitFact)) :ruleset s)
(rule ((A x) (B x)) ((C x)) :ruleset s)
(rule ((C x) (UnitFact)) ((Out x)) :ruleset s)
(rule () ((B (N 1))) :ruleset late)
(A (N 0)) (B (N 0)) (A (N 1))
(run-schedule (run s) (run late) (run s) (run s))'''
CASES['late-alias']='''(datatype E (N i64) (Box E)) (relation P (E)) (relation Q (E)) (relation R (E)) (ruleset s) (ruleset alias)
(rule ((P x) (Q x)) ((Box x) (R x)) :ruleset s)
(rule () ((union (N 0) (N 1))) :ruleset alias)
(P (N 0)) (Q (N 1)) (run-schedule (run s) (run alias) (run s))'''

CASES['ordinary-large']='''(datatype E (N i64)) (relation Seed (E))
(function Count () i64 :merge (+ old new)) (set (Count) 0) (ruleset s)
(rule ((Seed x)) ((set (Count) 1)) :ruleset s)
'''+''.join(f'(Seed (N {i}))\n' for i in range(1537))+'(run s 1)'
CASES['empty-grounded']='''(datatype E (N i64)) (ruleset s)
(rule () ((N 7)) :ruleset s) (run s 1)'''

def main():
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--bench',type=int,default=0);p.add_argument('--case',action='append');p.add_argument('--pass3-core',type=Path);p.add_argument('--reuse-control',type=Path);p.add_argument('--audit-repeat',action='store_true');a=p.parse_args()
    a.output.mkdir(parents=True,exist_ok=False);a.build=a.build.resolve();results={}
    if a.pass3_core:
        c=json.loads(a.pass3_core.read_text());plan=from_core(c)
        if a.reuse_control:
            old=json.loads((a.reuse_control/'pass3-on/program-0/cell-0/core.json').read_text())
            if old!=c:raise ValueError('historical control differs in typed source or schedule')
            previous=json.loads((a.reuse_control/'results.json').read_text())['True']
            results['historical_control']=dict(directory=str(a.reuse_control.resolve()),**previous)
        folder=a.output/'pass3-on';r=run(c,plan,a.build,folder,True,False,1048576,262144)
        results['candidate']=dict(seconds=r['programs'][0]['inference_seconds'],work=execution_summary(r),meta=r['programs'][0]['instances'][0]['meta'],worker=r['worker_provenance'],scope='full original resident input; general capabilities restored; no new source marker introduced')
        (a.output/'results.json').write_text(json.dumps(results,indent=2)+'\n');print('pass3',results['candidate']['seconds'],flush=True)
        if a.audit_repeat:
            first=run(c,plan,a.build,a.output/'checked',True,True,1048576,262144)
            repeated=run(c,plan,a.build,a.output/'repeat',True,True,1048576,262144)
            results['verification']=verify_repeat(first,repeated)
            (a.output/'results.json').write_text(json.dumps(results,indent=2)+'\n');print('pass3 checked repeat passed',flush=True)
        return
    for name,text in CASES.items():
        if a.case and name not in a.case:continue
        src=a.output/(name+'.egg');src.write_text(text);core=core_for(src,a.build);plan=from_core(core)
        lane={}
        for on in [False,True]:
            folder=a.output/(name+('-on' if on else '-off'));r=run(core,plan,a.build,folder,on)
            checked=verify(src,a.build,folder/'program-0/cell-0',a.output/(name+('-native-on' if on else '-native-off')))
            lane[str(on)]=dict(work=execution_summary(r),native=checked)
            if name=='ordinary-large' and on:assert r['programs'][0]['attempts'][0]['batch_context_growths']==0
            if on:
                repeated=run(core,plan,a.build,a.output/(name+'-repeat'),True)
                lane['True']['repeat']=verify_repeat(r,repeated)
        if name=='merge-resume':assert lane['True']['work']['merge_index_refreshes']>0,lane
        if name in ('ordinary-finite','ordinary-large'):assert lane['True']['work']['ordinary_rhs_bindings']>0,lane
        results[name]=lane;(a.output/'results.json').write_text(json.dumps(results,indent=2)+'\n');print(name,'passed',flush=True)
    if a.bench:
        src=a.output/'bench.egg';src.write_text(merge_source(a.bench));core=core_for(src,a.build);plan=from_core(core);pair={}
        for on in [False,True]:
            folder=a.output/('bench-on' if on else 'bench-off');r=run(core,plan,a.build,folder,on,audit=False)
            checked=verify(src,a.build,folder/'program-0/cell-0',a.output/('bench-native-on' if on else 'bench-native-off'))
            pair[str(on)]=dict(seconds=r['programs'][0]['inference_seconds'],work=execution_summary(r),native=checked,worker=r['worker_provenance']['binary_sha256'])
        assert pair['True']['worker']==pair['False']['worker'];results['bench']=pair
        (a.output/'results.json').write_text(json.dumps(results,indent=2)+'\n');print('bench',[(k,v['seconds']) for k,v in pair.items()],flush=True)
if __name__=='__main__':main()

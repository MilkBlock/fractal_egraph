"""Actual resident GPU keyed Cell publication; correctness and one-shot pairs."""
import argparse,copy,json,sys,time,hashlib
from pathlib import Path
from unittest.mock import patch
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
from test_generic_cells import core_for
from eggfuse.analyzer.regions import from_core
from eggfuse.engine.regions import verify_repeat,execution_summary
from verify_native_source import verify
import run_generic_cells as runtime

BASE='''(datatype E (N i64) (Mid E i64) (End E i64))
(relation Seed (E i64)) (function Cost (E) i64 :merge (min old new))
(ruleset a) (ruleset b)
(rule ((Seed root n)) ((let m (Mid root (+ n 1))) (set (Cost m) (+ n 1))) :ruleset a)
(rule ((= m (Mid root n)) (= c (Cost m)) (< c 1000000)) ((End m c)) :ruleset b)
'''

def timed(core,plan,build,folder,on,audit):
    lower=runtime.lower
    with patch.object(runtime,'lower',lambda *args,**kw:lower(*args,**kw,parallel_cell_waves=on)):
        c=copy.deepcopy(core);seed=c.pop('initial')
        lib=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=seed)])
        r=runtime.run_library(lib,build,folder,capacity=max(8192,8*len(seed)),match_capacity=max(4096,2*len(seed)),region_program=plan,audit_numeric=audit)
    assert r['complete'],r.get('rejected')
    p=json.loads((folder/'program-0/cell-0/program.json').read_text())
    assert p.get('cell_waves',{}).get('requested',p.get('cell_waves',{}).get('enabled')) is on,'new backend was not actually selected'
    return r


def main():
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--bench',type=int,default=0);p.add_argument('--pass3-core',type=Path);p.add_argument('--reuse-off',type=Path);p.add_argument('--historical-worker',action='store_true');a=p.parse_args()
    a.output.mkdir(parents=True,exist_ok=False);a.build=a.build.resolve();summary={}
    if a.pass3_core:
        c=json.loads(a.pass3_core.read_text());plan=from_core(c);lower=runtime.lower
        # A full program run with sufficient arena capacity is separate from
        # small fixture checks and excludes shader compilation and export.
        if a.reuse_off:
            previous=json.loads((a.reuse_off/'results.json').read_text())
            old_core=json.loads((a.reuse_off/'pass3-off/program-0/cell-0/core.json').read_text())
            if old_core!=c:raise ValueError('historical control has different source/control')
            summary['False']={**previous['False'],'historical':str(a.reuse_off.resolve())}
        for on in ([True] if a.reuse_off else [False,True]):
            with patch.object(runtime,'lower',lambda *args,**kw:lower(*args,**kw,parallel_cell_waves=on)):
                x=copy.deepcopy(c);seed=x.pop('initial');lib=dict(schema='analyzer-device-cells/v1',programs=[x],instances=[dict(unit=0,program=0,initial=seed)])
                folder=a.output/('pass3-on' if on else 'pass3-off');r=runtime.run_library(lib,a.build,folder,1048576,262144,region_program=plan)
            assert r['complete'],r['rejected']
            if a.reuse_off and summary['False']['worker']!=r['worker_provenance']['binary_sha256'] and not a.historical_worker:raise ValueError('historical control worker differs; use an explicitly labeled historical comparison')
            cp=json.loads((folder/'program-0/cell-0/program.json').read_text());assert cp['cell_waves']['enabled'] is on
            summary[str(on)]=dict(historical_control=a.historical_worker,inference_seconds=r['programs'][0]['inference_seconds'],work=execution_summary(r),state_meta=r['programs'][0]['instances'][0]['meta'],worker=r['worker_provenance']['binary_sha256'])
            (a.output/'results.json').write_text(json.dumps(summary,indent=2));print(on,summary[str(on)]['inference_seconds'],flush=True)
        return
    cases={
        'independent':BASE+''.join(f'(Seed (N {i}) {i})\n' for i in range(257))+'(run-schedule (saturate (run a) (run b)))',
        'duplicate-keys':BASE.replace('(Mid root (+ n 1))','(Mid (N 0) 0)')+''.join(f'(Seed (N {i}) {i})\n' for i in range(257))+'(run-schedule (saturate (run a) (run b)))',
        'constructor-union':'''(datatype E (N i64) (Box E)) (relation Seed (E E)) (relation Done (E))
(ruleset s) (rule ((Seed x y)) ((union (Box x) y)) :ruleset s)
(rule ((= b (Box x))) ((Done b)) :ruleset s)
'''+''.join(f'(Seed (N 0) (N {i+1}))\n' for i in range(129))+'(run-schedule (saturate s))',
        'noncommuting-fallback':'''(datatype E (N i64) (Choice E E)) (relation Seed (E E))
(function F (E) E :merge (Choice old new)) (relation Done (E E)) (ruleset s)
(rule ((Seed x y)) ((set (F x) y)) :ruleset s)
(rule ((= c (Choice x y))) ((Done x y)) :ruleset s)
(Seed (N 0) (N 1)) (Seed (N 0) (N 2)) (run-schedule (saturate s))'''
    }
    for name,text in cases.items():
        src=a.output/(name+'.egg');src.write_text(text);core=core_for(src,a.build);plan=from_core(core)
        on=timed(core,plan,a.build,a.output/(name+'-on'),True,True)
        again=timed(core,plan,a.build,a.output/(name+'-repeat'),True,True);verify_repeat(on,again)
        check=verify(src,a.build,a.output/(name+'-on/program-0/cell-0'),a.output/(name+'-native'))
        work=execution_summary(on)
        if name!='noncommuting-fallback':assert work['wave_effect_requests']>0,work
        else:assert work['wave_effect_requests']==0
        summary[name]=dict(verification=check,repeatable=True,work=work)
        (a.output/'results.json').write_text(json.dumps(summary,indent=2));print(name,'checked',flush=True)
    if a.bench:
        text=BASE+''.join(f'(Seed (N {i}) {i%1000})\n' for i in range(a.bench))+'(run-schedule (saturate (run a) (run b)))'
        src=a.output/'bench.egg';src.write_text(text);core=core_for(src,a.build);plan=from_core(core)
        pair={}
        for on in [False,True]:
            folder=a.output/('bench-on' if on else 'bench-off');r=timed(core,plan,a.build,folder,on,False)
            check=verify(src,a.build,folder/'program-0/cell-0',a.output/('bench-on-native' if on else 'bench-off-native'))
            pair[str(on)]=dict(inference_seconds=r['programs'][0]['inference_seconds'],work=execution_summary(r),verification=check,worker=r['worker_provenance']['binary_sha256'])
        assert pair['False']['worker']==pair['True']['worker'];summary['bench']=pair
        (a.output/'results.json').write_text(json.dumps(summary,indent=2));print('bench',[(k,v['inference_seconds']) for k,v in pair.items()],flush=True)

if __name__=='__main__':main()

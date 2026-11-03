"""Native semantics, stable replay and failure checks for resident RHS packets."""
import argparse,copy,json,sys,subprocess
from pathlib import Path
from unittest.mock import patch
ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend'),str(ROOT/'experiments/resident_restoration')]
from experiments.resident_restoration.check import run
from test_generic_cells import core_for
from eggfuse.analyzer.regions import from_core
from eggfuse.engine.regions import execution_summary,verify_repeat
from verify_native_source import verify
import run_generic_cells as runtime

N=257
BASE='''(datatype E (N i64) (P E i64) (Q E) (Pair E E))
(relation Seed (E i64)) (ruleset s)
'''
SEEDS=''.join(f'(Seed (N {i}) {i})\n' for i in range(N))
CASES={
'nested': BASE+'(rule ((Seed x n)) ((Q (P x n)) (Pair (P x n) x)) :ruleset s)'+SEEDS+'(run s 1)',
'duplicates': BASE+'(rule ((Seed x n)) ((Q (P x (% n 4))) (Pair (P x (% n 4)) x)) :ruleset s)'+''.join(f'(Seed (N 0) {i})' for i in range(N))+'(run s 1)',
'ordered-merge': BASE+'''(function F () E :merge (Pair old new)) (set (F) (N -1))
(rule ((Seed x n)) ((set (F) (Q (P x n)))) :ruleset s)'''+SEEDS+'(run s 1)',
'callback-overlap': BASE+'''(function F () E :merge (Pair old new)) (set (F) (N -1))
(rule ((Seed x n)) ((set (F) (Pair x x))) :ruleset s)'''+SEEDS+'(run s 1)',
'prediction': BASE+'''(rule ((Seed x n)) ((union (P x n) x)) :ruleset s)
(rule ((Seed x n)) ((Q (P x n))) :ruleset s)'''+SEEDS+'(run s 1)',
'set-prediction': BASE+'''(rule ((Seed x n)) ((union x (P x n))) :ruleset s)
(rule ((Seed x n)) ((Q (P x n))) :ruleset s)'''+SEEDS+'(run s 1)',
'late-union': BASE+'''(ruleset alias) (relation Seen (E))
(rule ((Seed x n)) ((Q (P x n))) :ruleset s)
(rule ((= a (N 0)) (= b (N 1))) ((union a b)) :ruleset alias)
(rule ((= q (Q (P x n))) (= x (N 0))) ((Seen q)) :ruleset s)
'''+SEEDS+'(run-schedule (run s) (run alias) (run s))',
'cell-cone': BASE+'''(ruleset next) (ruleset last)
(rule ((Seed x n)) ((P x n)) :ruleset s)
(rule ((= p (P x n))) ((Q p)) :ruleset next)
(rule ((= q (Q p))) ((Pair q q)) :ruleset last)
'''+SEEDS+'(run-schedule (saturate (run s) (run next) (run last)))',
'finite-visibility': BASE+'''(relation Early (E)) (ruleset observe)
(rule ((Seed x n)) ((Q (P x n))) :ruleset s)
(rule ((= q (Q p))) ((Pair q q)) :ruleset s)
(rule ((= t (Pair q q))) ((Early t)) :ruleset observe)
'''+SEEDS+'(run-schedule (run s) (run observe))',
'hidden-intermediate': BASE+'''(ruleset hide) (ruleset next)
(rule ((= p (P x n))) ((subsume (P x n))) :ruleset hide)
(rule ((Seed x n)) ((P x n)) :ruleset s)
(rule ((= p (P x n))) ((Q p)) :ruleset next)
'''+SEEDS+''.join(f'(P (N {i}) {i})' for i in range(N))+'(run-schedule (run hide) (saturate (run s) (run next)))',
}


def main():
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--case',action='append');a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False);a.build=a.build.resolve();results={}
    for name,text in CASES.items():
        if a.case and name not in a.case:continue
        src=a.output/(name+'.egg');src.write_text(text);core=core_for(src,a.build);plan=from_core(core);runs={};checks={}
        for on in (False,True):
            folder=a.output/(name+('-on' if on else '-off'))
            runs[on]=run(core,plan,a.build,folder,parallel_rhs_packets=on)
            checks[str(on)]=verify(src,a.build,folder/'program-0/cell-0',a.output/(name+('-native-on' if on else '-native-off')))
        work=execution_summary(runs[True]);state=runs[True]['programs'][0]['instances'][0]
        if name in ('nested','duplicates','finite-visibility'):
            old=runs[False]['programs'][0]['instances'][0]
            for k in ('rows','parents','fact_parents','sets','watermarks','meta','history'):assert old[k]==state[k],(name,k)
            assert work['packet_requests']>0
        if name=='ordered-merge':assert work['packet_ordered_tails']==N,work
        if name=='callback-overlap':assert work['packet_requests']==0,work
        if name=='cell-cone':assert work['packet_cell_steps']>0,work
        if name in ('hidden-intermediate','set-prediction'):assert work['packet_fallbacks']>0,work
        repeat=run(core,plan,a.build,a.output/(name+'-repeat'),parallel_rhs_packets=True)
        stable=verify_repeat(runs[True],repeat)
        results[name]=dict(native=checks,repeat=stable,work=work,seconds={str(k):v['programs'][0]['inference_seconds'] for k,v in runs.items()})
        (a.output/'results.json').write_text(json.dumps(results,indent=2)+'\n');print(name,'passed', {k:v for k,v in work.items() if k.startswith('packet_')},flush=True)
    if not a.case:
        # Writer capacity growth can interrupt one arena while another has an
        # unpublished dependency level. Restart only private preparation.
        src=a.output/'nested.egg';core=core_for(src,a.build);plan=from_core(core);initial=core.pop('initial');small=[];seen=False
        for row in initial:
            if row['table']=='Seed':
                if seen:continue
                seen=True
            small.append(copy.deepcopy(row))
        lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=small),dict(unit=1,program=0,initial=initial)])
        rr=[runtime.run_library(lib,a.build,a.output/('neighbors-'+x),8192,4096,region_program=plan,audit_numeric=True) for x in ('first','repeat')]
        verify_repeat(*rr);verify(src,a.build,a.output/'neighbors-first/program-0/cell-1',a.output/'neighbors-native')
        tid=next(i for i,t in enumerate(core['tables']) if t['name']=='Q');assert [sum(row[0]==tid for row in s['rows']) for s in rr[0]['programs'][0]['instances']]==[1,N]
        assert rr[0]['programs'][0]['attempts'][0]['write_growths']>0
        results['neighbor-growth']=dict(native=True,repeatable=True,independent=True)
        src=a.output/'overflow.egg';src.write_text(BASE+'(rule ((Seed x n)) ((Q (P x n)) (P x (+ n 1))) :ruleset s)'+SEEDS+'(Seed (N 0) 9223372036854775807) (run s 1)')
        c=core_for(src,a.build);plan=from_core(c);initial=c.pop('initial');lib=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=initial)])
        rr=[];lower=runtime.lower
        for on in (False,True):
            with patch.object(runtime,'lower',lambda *args,**kw:lower(*args,**kw,parallel_rhs_packets=on)):
                rr.append(runtime.run_library(lib,a.build,a.output/('overflow-'+str(on)),8192,4096,region_program=plan,audit_numeric=True))
        x,y=[r['programs'][0]['instances'][0] for r in rr];assert x['meta'][4]==y['meta'][4]==5
        for k in ('rows','parents','fact_parents','meta'):assert x[k]==y[k],k
        assert y['batch_statistics'][65]>0
        native=subprocess.run([str(a.build/'frontend'),str(src),'--native-full','--export-to',str(a.output/'overflow-native')],text=True,capture_output=True);assert native.returncode!=0
        results['overflow']=dict(same_failure_and_partial_state=True)
        (a.output/'results.json').write_text(json.dumps(results,indent=2)+'\n')
if __name__=='__main__':main()

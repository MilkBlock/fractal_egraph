"""Batch proof obligations + fresh-process per-Run GPU reproducibility.

No whole-program CPU endpoint equivalence requirement. Positive fixtures have
independent arithmetic/absence checks; source-rule validation is always active
in the GPU block publisher and exact numeric audit is enabled here.
"""
import argparse,copy,json,sys,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
from test_generic_cells import core_for
from run_generic_cells import run_library
from eggfuse.analyzer.regions import from_core
from eggfuse.engine.regions import verify_repeat,execution_summary
from eggfuse.ir.recipe import build,digest
from eggfuse.ir.batches import validate_batch


def rows(result,folder,table):
    p=json.loads((folder/'program-0/cell-0/program.json').read_text());i=result['programs'][0]['instances'][0]
    tid=next(j for j,t in enumerate(p['tables']) if t['name']==table)
    width=len(p['tables'][tid]['schema']);return [r[4:4+width] for r in i['rows'] if r[0]==tid and r[1]==0]


def check(build_dir,out,only=None,real_dir=None):
    out.mkdir(parents=True,exist_ok=False);base=(ROOT/'experiments/staged_batches/chain.egg').read_text().split('(Pair (N 0)')[0]
    schedule='(run-schedule (saturate (run build) (run advance) (run finish)))'
    wide=base+''.join(f'(Pair (N {2*i}) (N {2*i+1})) (set (Cost (N {2*i})) {i%100-50}) (set (Cost (N {2*i+1})) {i%80-40})\n' for i in range(513))+schedule
    cases={
      'wide':wide,
      'contended-key':base.replace('(set (Best r) n)','(set (Best (N 0)) n)')+''.join(f'(Pair (N {2*i}) (N {2*i+1})) (set (Cost (N {2*i})) {i}) (set (Cost (N {2*i+1})) 3)\n' for i in range(17))+schedule,
      'min-column':base.replace('(+ 1 (+ a b))','(min a b)')+'(Pair (N 0) (N 1)) (set (Cost (N 0)) -7) (set (Cost (N 1)) 3)'+schedule,
      'large-i64':base+'(Pair (N 0) (N 1)) (set (Cost (N 0)) 10000000000000000) (set (Cost (N 1)) 3)'+schedule,
      'finite':base+'(Pair (N 0) (N 1)) (set (Cost (N 0)) 3) (set (Cost (N 1)) 4) (run build 1)',
      'lost-min':'''(datatype E (N i64)) (function Best (E) i64 :merge (min old new))
(relation Offer (E i64)) (relation Candidate (E i64)) (relation Wrong (E i64))
(ruleset a) (ruleset b)
(rule ((Offer r n)) ((set (Best r) n) (Candidate r n)) :ruleset a)
(rule ((= n (Best r)) (> n 0)) ((Wrong r n)) :ruleset b)
(Offer (N 0) 5) (set (Best (N 0)) 0)
(run-schedule (saturate (run a) (run b)))''',
      'bound-guard':'''(datatype E (N i64)) (relation Seed (E i64 i64)) (relation Mid (E i64)) (relation Wrong (E i64))
(ruleset a) (ruleset b)
(rule ((Seed r x y)) ((Mid r x)) :ruleset a)
(rule ((Seed r x y) (Mid r z) (= z (+ x y))) ((Wrong r z)) :ruleset b)
(Seed (N 0) 5 1) (run-schedule (saturate (run a) (run b)))''',
      'overflow-merge':'''(datatype E (N i64)) (relation Seed (E i64))
(function Total (E) i64 :merge (+ old new)) (ruleset a)
(rule ((Seed x n)) ((set (Total x) n)) :ruleset a)
(Seed (N 0) 1) (set (Total (N 0)) 9223372036854775807)
(run-schedule (saturate (run a)))''',
      'masked-overflow':'''(datatype E (N i64)) (relation Seed (E i64)) (relation Mid (E i64)) (relation Wrong (E i64))
(ruleset a) (ruleset b)
(rule ((Seed r n)) ((Mid r n)) :ruleset a)
(rule ((Mid r n) (< n 0)) ((Wrong r (+ n 1))) :ruleset b)
(Seed (N 0) 9223372036854775807) (run-schedule (saturate (run a) (run b)))''',
      'constructor-resume':'''(datatype E (N i64) (Edge E)) (relation Seed (E E)) (relation Seen (E)) (relation Done (E))
(ruleset a) (ruleset b) (ruleset c)
(rule ((Seed x y)) ((union (Edge x) y)) :ruleset a)
(rule ((= y (Edge x))) ((Seen y)) :ruleset b)
(rule ((Seen y)) ((Done y)) :ruleset c)
(Seed (N 0) (N 2)) (Edge (N 0))
(run-schedule (saturate (run a) (run b) (run c)))''',
      'compacting-return':'''(datatype E (N i64) (K E) (Box E) (Gate E)) (relation Seed (E E E)) (relation Seen (E E))
(ruleset a) (ruleset b)
(rule ((Seed x y w)) ((K x) (K y) (Box w) (union x y) (union (Gate w) w)) :ruleset a)
(rule ((= v (Gate w))) ((Seen v (Box w))) :ruleset b)
(Seed (N 0) (N 1) (N 2)) (Gate (N 2))
(run-schedule (saturate (run a) (run b)))''',
      'retired-continuation':'''(datatype E (N i64) (Edge E)) (relation Seed (E E)) (relation Kill (E)) (relation Done (E))
(ruleset a) (ruleset b)
(rule ((Seed x y)) ((union (Edge x) y)) :ruleset a)
(rule ((Kill x)) ((subsume (Edge x))) :ruleset a)
(rule ((= y (Edge x))) ((Done y)) :ruleset b)
(Seed (N 0) (N 2)) (Kill (N 0)) (Edge (N 0))
(run-schedule (saturate (run a) (run b)))''',
      'shared-identity':'''(datatype E (N i64) (Shared E)) (relation Seed (E i64)) (relation Seen (E E)) (relation Done (E))
(ruleset a) (ruleset b)
(rule ((Seed x n)) ((Seen x (Shared x))) :ruleset a)
(rule ((Seen x y)) ((Done y)) :ruleset b)
(Seed (N 0) 1) (Seed (N 0) 2)
(run-schedule (saturate (run a) (run b)))''',
      'actual-min-port':'''(datatype E (N i64)) (relation Seed (E i64)) (function Best (E) i64 :merge (min old new)) (relation Done (E i64))
(ruleset a) (ruleset b)
(rule ((Seed x n)) ((set (Best x) n)) :ruleset a)
(rule ((= n (Best x))) ((Done x (+ n 1))) :ruleset b)
(Seed (N 0) 12) (set (Best (N 0)) 3)
(run-schedule (saturate (run a) (run b)))''',
      'custom-chain':'''(datatype E (N i64) (Choice E E)) (relation Seed (E E)) (function F (E) E :merge (Choice old new)) (relation Done (E E))
(ruleset a) (ruleset b)
(rule ((Seed x y) (= old (F x))) ((set (F x) y)) :ruleset a)
(rule ((= c (Choice left right))) ((union c right) (Done left right)) :ruleset b)
(Seed (N 0) (N 1)) (set (F (N 0)) (N 0))
(run-schedule (saturate (run a) (run b)))''',
      'custom-merge':'''(datatype E (N i64) (Choice E E)) (relation Seed (E E E)) (function F (E) E :merge (Choice old new)) (relation Done (E E))
(ruleset a) (ruleset b)
(rule ((Seed x y z)) ((set (F x) y) (set (F x) z)) :ruleset a)
(rule ((= c (Choice left right))) ((Done left right)) :ruleset b)
(Seed (N 0) (N 1) (N 2)) (Seed (N 0) (N 3) (N 4))
(run-schedule (saturate (run a) (run b)))''',
      'union-and-subsume':'''(datatype E (N i64) (Edge E E)) (relation Seen (E)) (relation Done (E))
(ruleset observe) (ruleset edit)
(rule ((= e (Edge x y))) ((Seen e)) :ruleset observe)
(rule ((Seen e)) ((Done e)) :ruleset observe)
(rule ((= e (Edge x y))) ((union x y) (subsume (Edge x y))) :ruleset edit)
(Edge (N 0) (N 1)) (run-schedule (saturate (run observe) (run edit)))'''
    }
    cases['independent-unions']=cases['constructor-resume'].replace('(Seed (N 0) (N 2)) (Edge (N 0))', ''.join(f'(Seed (N {3*i}) (N {3*i+2})) (Edge (N {3*i}))' for i in range(129)))
    if real_dir:
        for name in ['finite','late-union','context-to-terms','renamed']:
            cases['real-'+name]=(real_dir/(name+'.egg')).read_text()
    if only:
        missing=set(only)-set(cases)
        if missing:raise ValueError('unknown fixture '+str(missing))
        cases={k:v for k,v in cases.items() if k in only}
    results={}
    for name,text in cases.items():
        source=out/(name+'.egg');source.write_text(text);core=core_for(source,build_dir);plan=from_core(core)
        validate_batch(plan['batch_program'],build(core),plan)
        if name=='wide':
            bad=copy.deepcopy(plan['batch_program']);t=next(t for t in bad['templates'] if len(t['steps'])==3)
            step=t['steps'][0];r=build(core)['rules'][step['recipe_rule']]
            dst=next(i for i,s in enumerate(r['slots']) if s['definition']!='query' and s['sort']['kind']=='i64')
            input_slot=next(i for i,s in enumerate(t['slots'][:t['input_width']]) if s['sort']['kind']=='i64')
            step['mapping'][dst]={'slot':input_slot};bad['sha256']=digest({k:v for k,v in bad.items() if k!='sha256'})
            try:validate_batch(bad,build(core),plan)
            except ValueError:pass
            else:raise AssertionError('SSA overwrite accepted')
        initial=core.pop('initial');lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
        runs=[run_library(lib,build_dir,out/(name+'-'+suffix),capacity=8192,match_capacity=4096,region_program=plan,audit_numeric=True) for suffix in ['first','repeat']]
        if name=='overflow-merge':
            states=[r['programs'][0]['instances'][0] for r in runs]
            assert states[0]==states[1] and states[0]['meta'][4]==5
            native=subprocess.run([str(build_dir/'frontend'),str(source),'--native-full','--export-to',str(out/'native-overflow')],text=True,capture_output=True)
            assert native.returncode!=0
            (out/'native-overflow.log').write_text(native.stdout+native.stderr)
            results[name]=dict(expected_source_error='i64 merge overflow',device_error=5,native_failed=True,exact_repeated_private_state=True)
            (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');print(name,'expected error and repeated state verified',flush=True)
            continue
        verified=verify_repeat(*runs);summary=execution_summary(runs[0]);folder=out/(name+'-first')
        if name=='wide':
            done=rows(runs[0],folder,'Done');expected=sorted(i%100+i%80-87 for i in range(513))
            assert sorted(r[1] for r in done)==expected
            assert summary['validated_successor_invocations']>=1026 and summary['matrix_context_rows']>=513
            assert summary['max_tile_rows']==128
            assert summary['completed_binding_shortcuts']>=1026
            assert summary['batch_seed_invocations']==513
            assert any(a['batch_context_growths']>0 for a in runs[0]['programs'][0]['attempts'])
        if name=='contended-key':assert [r[1] for r in rows(runs[0],folder,'Best')]==[6]
        if name=='min-column':assert [r[1] for r in rows(runs[0],folder,'Done')]==[-5]
        if name=='large-i64':
            assert [r[1] for r in rows(runs[0],folder,'Middle')]==[10000000000000004]
            assert not rows(runs[0],folder,'Done') and summary['matrix_context_rows']==0
        if name=='finite':assert summary['validated_successor_invocations']==0
        if name in ['lost-min','bound-guard','masked-overflow']:assert not rows(runs[0],folder,'Wrong')
        if name=='lost-min':assert summary['rejected_successor_invocations']>0
        if name=='independent-unions':
            assert len(rows(runs[0],folder,'Done'))==129
            assert summary['continuation_blocks']<=2 and summary['validated_successor_invocations']==258
        if name=='constructor-resume':
            assert rows(runs[0],folder,'Done')
            assert summary['continuation_blocks']>0 and summary['constructor_set_requests']>0
            assert summary['validated_successor_invocations']>0
        if name=='compacting-return':
            box=rows(runs[0],folder,'Box')[0][-1]
            assert all(r[1]==box for r in rows(runs[0],folder,'Seen')), 'constructor prediction survived row compaction with wrong identity'
        if name=='retired-continuation':
            assert not rows(runs[0],folder,'Done')
            assert summary['rejected_successor_invocations']>0
        if name=='shared-identity':
            assert len(rows(runs[0],folder,'Shared'))==1 and len(rows(runs[0],folder,'Done'))==1
        if name=='actual-min-port':
            assert [r[1] for r in rows(runs[0],folder,'Done')]==[4]
            assert summary['validated_successor_invocations']>0
        if name=='custom-chain':
            assert summary['validated_successor_invocations']>0 and summary['union_merge_blocks']>0
        if name=='custom-merge':
            assert len(rows(runs[0],folder,'Choice'))==3
            assert summary['custom_merge_requests']>=4
            assert len(rows(runs[0],folder,'Done'))==3 # every correlated callback, not just a winner
        results[name]=dict(verification=verified,execution=summary,templates=len(plan['batch_program']['templates']))
        (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');print(name,summary,flush=True)
    return results

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--case',action='append');p.add_argument('--real-dir',type=Path);a=p.parse_args();check(a.build.resolve(),a.output.resolve(),a.case,a.real_dir)

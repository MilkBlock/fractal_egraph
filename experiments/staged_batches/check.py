"""Batch proof obligations + fresh-process per-Run GPU reproducibility.

No whole-program CPU endpoint equivalence requirement. Positive fixtures have
independent arithmetic/absence checks; source-rule validation is always active
in the GPU block publisher and exact numeric audit is enabled here.
"""
import argparse,copy,json,sys
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


def check(build_dir,out):
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
      'masked-overflow':'''(datatype E (N i64)) (relation Seed (E i64)) (relation Mid (E i64)) (relation Wrong (E i64))
(ruleset a) (ruleset b)
(rule ((Seed r n)) ((Mid r n)) :ruleset a)
(rule ((Mid r n) (< n 0)) ((Wrong r (+ n 1))) :ruleset b)
(Seed (N 0) 9223372036854775807) (run-schedule (saturate (run a) (run b)))''',
      'union-and-subsume':'''(datatype E (N i64) (Edge E E)) (relation Seen (E)) (relation Done (E))
(ruleset observe) (ruleset edit)
(rule ((= e (Edge x y))) ((Seen e)) :ruleset observe)
(rule ((Seen e)) ((Done e)) :ruleset observe)
(rule ((= e (Edge x y))) ((union x y) (subsume (Edge x y))) :ruleset edit)
(Edge (N 0) (N 1)) (run-schedule (saturate (run observe) (run edit)))'''
    }
    results={}
    for name,text in cases.items():
        source=out/(name+'.egg');source.write_text(text);core=core_for(source,build_dir);plan=from_core(core)
        validate_batch(plan['batch_program'],build(core),plan)
        if name=='wide':
            bad=copy.deepcopy(plan['batch_program']);t=bad['templates'][-1]
            step=t['steps'][0];r=build(core)['rules'][step['recipe_rule']]
            dst=next(i for i,s in enumerate(r['slots']) if s['definition']!='query' and s['sort']['kind']=='i64')
            input_slot=next(i for i,s in enumerate(t['slots'][:t['input_width']]) if s['sort']['kind']=='i64')
            step['mapping'][dst]={'slot':input_slot};bad['sha256']=digest({k:v for k,v in bad.items() if k!='sha256'})
            try:validate_batch(bad,build(core),plan)
            except ValueError:pass
            else:raise AssertionError('SSA overwrite accepted')
        initial=core.pop('initial');lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
        runs=[run_library(lib,build_dir,out/(name+'-'+suffix),capacity=8192,match_capacity=4096,region_program=plan,audit_numeric=True) for suffix in ['first','repeat']]
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
        results[name]=dict(verification=verified,execution=summary,templates=len(plan['batch_program']['templates']))
        (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');print(name,summary,flush=True)
    return results

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();check(a.build.resolve(),a.output.resolve())

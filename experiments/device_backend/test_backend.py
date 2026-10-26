"""Real Metal/native differential tests. Not timing or acceleration claims."""
import argparse,json,subprocess
from pathlib import Path
from run import run,compare,atomic_json
HERE=Path(__file__).resolve().parent

def tests(build,out):
 out.mkdir(parents=True,exist_ok=True);results={}
 for name,src in [('merge-index-refresh',HERE/'fixtures/merge-index-refresh.egg'),('delta-join',HERE/'fixtures/delta-join.egg'),('deferred-union',HERE/'fixtures/deferred-union.egg'),('sets',HERE/'fixtures/sets.egg'),('merge-rebuild',HERE/'fixtures/merge-rebuild.egg'),('custom-merge',HERE/'fixtures/custom-merge.egg'),('merge-same',HERE/'fixtures/merge-same.egg'),('integer',HERE/'fixtures/integer.egg'),('float-identity',HERE/'fixtures/float-identity.egg'),('boolean',HERE/'fixtures/boolean.egg'),('pipeline',HERE/'pipeline.egg'),('subsume-absent',HERE/'fixtures/subsume-absent.egg'),('unit-function',HERE/'fixtures/unit-function.egg'),('initial-union',HERE/'fixtures/initial-union.egg'),('naive',HERE/'fixtures/naive.egg'),('repeat',HERE/'fixtures/repeat.egg'),('math',HERE/'fixtures/math.egg'),('combined',HERE/'fixtures/combined.egg'),('update',HERE/'fixtures/update.egg'),('subsume-union',HERE/'fixtures/subsume-union.egg'),('spine',HERE.parent/'cell_spine/manual_marker/long-after.egg')]:
  r=run(src,build,out/name);assert r['checks'] and all(c['exact_rows_values_equivalence_visibility'] for c in r['checks']);results[name]=r
 r=run(HERE/'fixtures/subsume-union.egg',build,out/'small-capacity-compaction',capacity=16,match_capacity=32);assert r['checks'];results['small-capacity-compaction']=r
 count=512
 source='(datatype T (N i64) (W T) (P T T)) (relation Link (T T)) (function Cost (T) i64 :merge (min old new)) (ruleset init) (ruleset fold)\n(rule () ('
 for i in range(count):source+=f'(let n{i} (N {i})) (W (W n{i})) (P n{i} (W n{i})) (Link n{i} (N 0)) (set (Cost n{i}) {i}) '
 source+=' ) :ruleset init) (rule ((Link x y)) ((union x y)) :ruleset fold) (run init 1) (run fold 1) (check (= (Cost (N 127)) 0)) (check (= (W (W (N 127))) (W (W (N 0)))))'
 stress=out/'congruence-stress.egg';stress.write_text(source)
 r=run(stress,build,out/'congruence-stress');assert r['steps'][1]['unions']>=4*(count-1);results['congruence-stress']=r
 binary=run(stress,build,out/'binary-radix',radix_digit_bits=1)
 assert binary['steps']==r['steps']
 assert json.loads((out/'binary-radix/gpu-state.json').read_text())==json.loads((out/'congruence-stress/gpu-state.json').read_text())
 results['stable-digit-radix']=True
 cart='(datatype T (N i64) (Pair T T)) (relation L (T)) (relation R (T)) (ruleset init) (ruleset product) (rule () ('
 for i in range(32):cart+=f'(L (N {i})) (R (N {i})) '
 cart+=') :ruleset init) (rule ((L x) (R y)) ((Pair x y)) :ruleset product) (run init 1) (run product 1)'
 path=out/'match-order.egg';path.write_text(cart)
 parallel=run(path,build,out/'match-order-parallel');serial=run(path,build,out/'match-order-serial',parallel_match=False)
 assert parallel['steps']==serial['steps'] and parallel['steps'][1]['matched']==1024
 assert json.loads((out/'match-order-parallel/gpu-state.json').read_text())==json.loads((out/'match-order-serial/gpu-state.json').read_text())
 results['parallel-binding-order']=True
 # Column pruning must not change the timestamp gates on unindexed seed
 # tables. Compare the entire state, including watermarks and physical order.
 all_columns=run(HERE/'fixtures/delta-join.egg',build,out/'all-index-columns',all_index_columns=True)
 assert all_columns['steps']==results['delta-join']['steps']
 assert json.loads((out/'all-index-columns/gpu-state.json').read_text())==json.loads((out/'delta-join/gpu-state.json').read_text())
 limited=json.loads((out/'delta-join/program.json').read_text())['index_columns']
 full=json.loads((out/'all-index-columns/program.json').read_text())['index_columns']
 assert sum(map(len,limited.values()))<sum(map(len,full.values()))
 results['used-index-columns']=True
 flat=run(HERE/'fixtures/delta-join.egg',build,out/'flat-dispatch',seed_dispatch=False)
 assert flat['steps']==results['delta-join']['steps']
 assert json.loads((out/'flat-dispatch/gpu-state.json').read_text())==json.loads((out/'delta-join/gpu-state.json').read_text())
 results['typed-seed-dispatch']=True
 patched=run(HERE/'fixtures/query-value-updates.egg',build,out/'query-value-updates',capacity=1024)
 rebuilt=run(HERE/'fixtures/query-value-updates.egg',build,out/'query-value-updates-full',capacity=1024,query_patch=False)
 assert patched['steps']==rebuilt['steps']
 assert json.loads((out/'query-value-updates/gpu-state.json').read_text())==json.loads((out/'query-value-updates-full/gpu-state.json').read_text())
 results['point-index-updates']=patched
 serial_index=run(HERE/'fixtures/query-value-updates.egg',build,out/'query-value-updates-serial',capacity=1024,parallel_prepare=False,parallel_match=False)
 assert serial_index['steps']==patched['steps']
 assert json.loads((out/'query-value-updates/gpu-state.json').read_text())==json.loads((out/'query-value-updates-serial/gpu-state.json').read_text())
 results['point-index-updates-serial']=True
 for case in ['sets','integer','custom-merge']:
  bytecode=run(HERE/'fixtures'/(case+'.egg'),build,out/(case+'-bytecode'),direct_actions=False)
  assert bytecode['steps']==results[case]['steps']
  assert json.loads((out/(case+'-bytecode')/'gpu-state.json').read_text())==json.loads((out/case/'gpu-state.json').read_text())
  results['direct-actions-'+case]=True
 stable=run(HERE/'fixtures/stable-empty-rhs.egg',build,out/'stable-empty-rhs')
 assert stable['rhs_noops_skipped']==2
 reference=run(HERE/'fixtures/stable-empty-rhs.egg',build,out/'stable-empty-rhs-off',noop_preflight=False)
 assert stable['steps']==reference['steps']
 assert json.loads((out/'stable-empty-rhs/gpu-state.json').read_text())==json.loads((out/'stable-empty-rhs-off/gpu-state.json').read_text())
 results['stable-empty-rhs']=stable
 unstable=run(HERE/'fixtures/unstable-empty-rhs.egg',build,out/'unstable-empty-rhs')
 assert unstable['rhs_noops_skipped']==0
 results['reject-unstable-empty-rhs']=unstable
 repeated=run(HERE/'fixtures/repeated-effects.egg',build,out/'repeated-effects',effect_dedup=True)
 repeat_control=run(HERE/'fixtures/repeated-effects.egg',build,out/'repeated-effects-off',effect_dedup=False)
 assert repeated['duplicate_effects_skipped']==189 and repeated['rhs_noops_skipped']==1
 assert repeated['steps']==repeat_control['steps']
 assert json.loads((out/'repeated-effects/gpu-state.json').read_text())==json.loads((out/'repeated-effects-off/gpu-state.json').read_text())
 results['repeated-effects']=repeated
 ordered=run(HERE/'fixtures/ordered-effects.egg',build,out/'ordered-effects',effect_dedup=True)
 assert ordered['duplicate_effects_skipped']==0
 results['preserve-ordered-effects']=ordered
 snapshot=run(HERE/'fixtures/snapshot-subsume.egg',build,out/'snapshot-subsume')
 snapshot_vm=run(HERE/'fixtures/snapshot-subsume.egg',build,out/'snapshot-subsume-bytecode',direct_actions=False)
 assert snapshot['steps']==snapshot_vm['steps']
 assert json.loads((out/'snapshot-subsume/gpu-state.json').read_text())==json.loads((out/'snapshot-subsume-bytecode/gpu-state.json').read_text())
 results['snapshot-subsume']=snapshot
 prediction=run(HERE/'fixtures/set-prediction.egg',build,out/'set-prediction')
 results['separate-set-and-prediction']=prediction
 custom_duplicates=run(HERE/'fixtures/repeated-custom-set.egg',build,out/'repeated-custom-set',effect_dedup=True)
 assert custom_duplicates['duplicate_effects_skipped']==0
 results['preserve-repeated-custom-merge']=custom_duplicates
 guarded=run(HERE/'fixtures/seed-pivot-guards.egg',build,out/'seed-pivot-guards',early_seed_guards=True)
 late=run(HERE/'fixtures/seed-pivot-guards.egg',build,out/'seed-pivot-guards-late',early_seed_guards=False)
 assert guarded['steps']==late['steps']
 assert json.loads((out/'seed-pivot-guards/gpu-state.json').read_text())==json.loads((out/'seed-pivot-guards-late/gpu-state.json').read_text())
 results['early-seed-pivot-guards']=guarded
 t=stable['timings_seconds'];assert abs(t['rhs_commit']+t['parallel_rebuild']+(t['noop_preflight'] or 0)-t['commit'])<1e-7
 assert {'pure','mixed'}<=set(results['deferred-union']['deferred_rulesets'])
 # Eager union is a diagnostic counterfactual, and is known to lose a merge
 # constructor in this case. Never silently bless it as native-equivalent.
 try:run(HERE/'fixtures/deferred-union.egg',build,out/'eager-union',defer_unions=False)
 except AssertionError as e:
  assert 'row count differs' in str(e);results['reject-eager-union']={'expected_validation_failure':str(e)}
 else:raise AssertionError('eager union lost its regression witness')
 unit=json.loads((out/'unit-function/program.json').read_text());assert next(t for t in unit['tables'] if t['name']=='F')['kind']=='function'
 assert len(results['repeat']['steps'])==2,'Repeat must stop at a native quiescent boundary'
 assert [s['matched'] for s in results['naive']['steps']]==[1,1],'naive rules must not inherit seminaive watermarks'
 assert results['pipeline']['steps'][2]['unions']>=2,'explicit union must also cause congruence union'
 assert results['pipeline']['steps'][-1]['matched']==0,'hidden witness must not match'
 assert results['combined']['steps'][1]['matched']==0,'shared rule watermark must survive ruleset switch'
 assert results['update']['steps'][-2]['updated'] is False
 r=run(HERE.parent/'cell_spine/manual_marker/long-after.egg',build,out/'spine-unchecked',check_every=0)
 assert r['native_oracle_runs']==0 and r['checkpoint_bytes']==0 and r['native_retired_before_gpu_runs'];assert r['steps']==results['spine']['steps'];results['spine-unchecked']=r
 r=run(HERE/'pipeline.egg',build,out/'periodic',check_every=3);assert [c['step'] for c in r['checks']]==[3,6];results['periodic']=r
 r=run(HERE/'pipeline.egg',build,out/'final-check',check_at_end=True);assert r['oracle_starts_after_gpu'] and len(r['checks'])==1 and r['native_oracle_runs']==6;assert r['steps']==results['pipeline']['steps'];results['final-check']=r
 r=run(HERE/'pipeline.egg',build,out/'prefix',max_runs=2);assert not r['complete_schedule'] and len(r['steps'])==2 and r['checks'];results['prefix']=r
 r=run(HERE/'pipeline.egg',build,out/'resumed',resume=out/'prefix');assert r['complete_schedule'] and r['resumed_prefix_runs']==2 and r['steps']==results['pipeline']['steps'] and len(r['device_command_times'])==len(r['steps'])-2;results['resumed']=r
 r=run(HERE/'pipeline.egg',build,out/'stored-reference',verify_against=out/'pipeline');assert r['checked'] and r['native_oracle_runs']==0 and r['native_retired_before_gpu_runs'] and r['steps']==results['pipeline']['steps'];results['stored-reference']=r
 preserved=json.loads((out/'prefix/verified-gpu-state.json').read_text());bad=json.loads((out/'prefix/gpu-state.json').read_text());bad['rows'][0][1]=1-bad['rows'][0][1];atomic_json(out/'prefix/gpu-state.json',bad)
 assert json.loads((out/'prefix/verified-gpu-state.json').read_text())==preserved
 r=run(HERE/'pipeline.egg',build,out/'resume-preserved',resume=out/'prefix');assert r['steps']==results['pipeline']['steps'];results['preserved-checkpoint']=True
 r=run(HERE/'pipeline.egg',build,out/'gpu-prefix',check_every=0,max_runs=2,save_final_state=True,snapshot_every=1);assert r['native_oracle_runs']==0
 r=run(HERE/'pipeline.egg',build,out/'gpu-resumed',check_every=0,resume=out/'gpu-prefix',require_native_resume=False,save_final_state=True);assert not r['checked'] and r['native_oracle_runs']==0
 assert json.loads((out/'gpu-resumed/gpu-state.json').read_text())==json.loads((out/'pipeline/gpu-state.json').read_text());results['gpu-checkpoint-continuation']=True
 failures={
  'panic':('(datatype T (N i64)) (ruleset r) (N 0) (rule ((N 0)) ((panic "expected")) :ruleset r) (run r 1)',{},'error=8'),
  'match-capacity':('(datatype T (N i64) (W T)) (ruleset r) (rewrite (N n) (W (N n)) :ruleset r) (N 1) (N 2) (run r 1)',{'match_capacity':1},'error=2'),
  'row-capacity':('(datatype T (N i64) (W T)) (ruleset r) (rewrite (N n) (W (N n)) :ruleset r) (N 1) (N 2) (run r 1)',{'capacity':2},'error=1'),
  'source-check':('(datatype T (N i64)) (ruleset r) (N 1) (run r 1) (check (N 2))',{},'source check failed'),
  'unit-lookup':('(datatype T (N i64)) (function F (T) Unit :no-merge) (ruleset r) (rule ((= x (N 1))) ((F x)) :ruleset r :naive) (N 1) (run r 1)',{},'native compilation failed'),
  'ambiguous-old':('(datatype T (N i64)) (function F (T) i64 :merge old) (ruleset r) (rule ((= x (N 0))) ((set (F x) 1)) :ruleset r) (rule ((= x (N 0))) ((set (F x) 2)) :ruleset r) (N 0) (run r 1)',{},'error=7'),
  'overflow':('(datatype T (N i64)) (function F (T) i64 :merge (max old new)) (ruleset r) (rule ((= n (F x))) ((set (F x) (+ n 1))) :ruleset r) (set (F (N 0)) 9223372036854775807) (run r 1)',{},'error=5'),
 }
 for name,(source,options,error) in failures.items():
  path=out/(name+'.egg');path.write_text(source)
  try:run(path,build,out/name,**options)
  except (ValueError,RuntimeError,AssertionError) as e:assert error in str(e),(name,str(e));results[name]={'expected_rejection':str(e)}
  else:raise AssertionError(name+' accepted')
  assert not (out/name/'report.json').exists();assert (out/name/'failure.json').exists()
 native_error=subprocess.run([str(build.resolve()/'frontend'),str(out/'overflow.egg'),'--native-full','--export-to',str(out/'native-overflow')],capture_output=True,text=True)
 assert native_error.returncode!=0 and 'call of primitive + failed' in native_error.stderr
 results['native-rhs-overflow-contract']=True
 # Deliberately corrupt successful snapshots: equal counts are not enough.
 p=json.loads((out/'pipeline/program.json').read_text());g=json.loads((out/'pipeline/gpu-state.json').read_text());n=json.loads((out/'pipeline/native-state.json').read_text())
 for corruption in ['visibility','value','equivalence']:
  rows=[r[:] for r in g['rows']];parents=g['parents'][:]
  if corruption=='visibility':rows[0][1]=1-rows[0][1]
  if corruption=='value':
   row=next(r for r in rows if p['tables'][r[0]]['name']=='Cost');row[4+len(p['tables'][row[0]]['schema'])-1]+=1 # corrupt primitive result, not its key
  if corruption=='equivalence':
   roots=[i for i,v in enumerate(parents) if i>0 and i==v];parents[roots[-1]]=roots[0]
  try:compare(p,rows,parents,n)
  except (AssertionError,KeyError):results['reject-corrupt-'+corruption]=True
  else:raise AssertionError('accepted corrupt '+corruption)
 (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');return results
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();r=tests(a.build,a.output);print('passed',len(r),'checks; results:',a.output/'results.json')

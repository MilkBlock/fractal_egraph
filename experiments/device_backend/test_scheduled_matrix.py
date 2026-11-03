"""Native schedule, ordinary GPU, and private multi-stage matrix patches."""
import argparse,json,os,hashlib
from pathlib import Path
from run import run
from eggfuse.compiler.targets.metal.compiler import compile_program
from compare_runs import compare
from verify_native_source import verify
from test_joint_polyspine import rejected_transaction

BASE='''(datatype X (N i64) (Edge X X))
(relation Seed (X)) (relation P (X)) (relation Q (X))
(ruleset a) (ruleset b)
(rule ((Seed x)) ((P x)) :ruleset a)
(rule ((Edge x y) (Q x)) ((P y)) :ruleset a)
(rule ((P x)) ((Q x)) :ruleset b)
'''
def chain(n):return '(Seed (N 0))\n'+'\n'.join(f'(Edge (N {i}) (N {i+1}))' for i in range(n))+'\n'
def tests(build,out):
 out.mkdir(parents=True,exist_ok=True);results={}
 def pair(name,source):
  f=out/(name+'.egg');f.write_text(source)
  def lane(suffix,**opts):
   folder=out/(name+suffix);report=folder/'report.json'
   if report.exists():
    saved=json.loads(report.read_text())
    assert saved['build']['worker_sha256']==hashlib.sha256((build/'worker').read_bytes()).hexdigest()
    assert saved['source_sha256']==hashlib.sha256(f.read_bytes()).hexdigest()
    stored=json.loads((folder/'program.json').read_text());core=json.loads((folder/'core.json').read_text())
    current=compile_program(core,capacity=stored['capacity'],match_capacity=stored['match_capacity'],scheduled_matrix=opts.get('scheduled_matrix',False))
    assert current['shader']==stored['shader'],'cached shader differs; use a new output directory'
    return saved
   if folder.exists():folder.rename(folder.with_name(folder.name+'.failed-'+str(os.getpid())))
   return run(f,build,folder,check_at_end=True,capacity=16384 if name=='twenty-eight-stages' else 4096,**opts)
  a=lane('-ordinary');b=lane('-patch',scheduled_matrix=True)
  fields=['set','updated','matched','rows','epoch']
  assert [[s[k] for k in fields] for s in a['steps']]==[[s[k] for k in fields] for s in b['steps']],name
  check=compare(out/(name+'-ordinary'),out/(name+'-patch'),device_states=True,semantic_device=True)
  assert check['final_check']['exact_rows_values_equivalence_visibility']
  original=out/(name+'-source-native')
  native=json.loads((original/'verification.json').read_text()) if original.exists() else verify(f,build,out/(name+'-patch'),original)
  assert native['original_native_schedule']
  results[name]={'original_schedule_check':native,'ordinary_seconds':a['inference_seconds'],'patch_seconds':b['inference_seconds'],'check':check['final_check'],'calls':b['scheduled_matrix']['calls']}
  print(name,'passed',[(v['logical_runs'],v['patch_operand_reads']) for v in b['scheduled_matrix']['calls']],flush=True);return b
 b=pair('finite-five',BASE+chain(8)+'(run-schedule (repeat 5 (run a) (saturate (run b)))) (check (Q (N 4)))')
 assert len([s for s in b['steps'] if s['set']=='a'])==5
 assert b['scheduled_matrix']['calls'][0]['global_publications']==1
 b=pair('nested-fixedpoint',BASE+chain(19)+'(run-schedule (saturate (run a) (saturate (run b)))) (check (Q (N 19)))')
 assert b['scheduled_matrix']['calls'][0]['patch_operand_reads']>20
 pair('zero-repeat',BASE+chain(3)+'(run-schedule (repeat 0 (run a) (run b)))')
 pair('unseeded-cycle',BASE+'(Edge (N 0) (N 1)) (Edge (N 1) (N 0)) (run-schedule (saturate (run a) (run b)))')
 pair('ordinary-run-visibility',BASE+chain(3)+'''(relation Early ()) (ruleset observe)
(rule ((Q (N 1))) ((Early)) :ruleset observe)
(run a 1) (run b 1) (run observe 1)
(run-schedule (repeat 2 (run a) (run b))) (run observe 1) (check (Early))''')
 pair('union-boundary',BASE+'''(Seed (N 0)) (Edge (N 1) (N 2))
(ruleset merge) (rule ((= x (N 0)) (= y (N 1))) ((union x y)) :ruleset merge)
(run-schedule (saturate (run a) (run b))) (run merge 1)
(run-schedule (saturate (run a) (run b))) (check (Q (N 2)))''')
 pair('subsume-boundary',BASE+chain(3)+'''(ruleset hide)
(rule ((= x (N 1)) (Q x)) ((subsume (Q x))) :ruleset hide)
(run-schedule (repeat 2 (run a) (run b))) (run hide 1)
(run-schedule (saturate (run a) (run b)))''')
 pair('function-merge-boundary',BASE+chain(3)+'''(function Cost (X) i64 :merge (min old new)) (ruleset price)
(rule ((Q x)) ((set (Cost x) 9) (set (Cost x) 4)) :ruleset price)
(run-schedule (saturate (run a) (run b))) (run price 1) (check (= (Cost (N 3)) 4))''')
 pair('history-growth',BASE+chain(100)+'(run-schedule (saturate (run a) (saturate (run b)))) (check (Q (N 100)))')
 pair('until-control','(datatype X (N i64)) (ruleset a)\n(rule ((N n)) ((N (+ n 1))) :ruleset a)\n(N 0) (run-schedule (repeat 5 (run a :until (N 2)))) (check (N 2))')
 pair('until-refresh','(datatype X (Root)) (function F () i64 :merge new) (ruleset step) (ruleset reset)\n(rule ((= n (F))) ((set (F) (+ n 1))) :ruleset step)\n(rule ((= (F) 2)) ((set (F) 0)) :ruleset reset)\n(set (F) 0) (run-schedule (repeat 5 (run step :until (= (F) 2))) (run reset) (repeat 5 (run step :until (= (F) 2)))) (check (= (F) 2))')
 pair('private-name-collision','(datatype X (N i64) (__cell_identity X)) (relation P (X)) (ruleset __scheduled_matrix_0)\n(rule ((__cell_identity x)) ((P x)) :ruleset __scheduled_matrix_0)\n(__cell_identity (N 1)) (N 2) (run-schedule (saturate (run __scheduled_matrix_0))) (check (P (N 1)))')
 pair('late-producer-region','(datatype X (N i64) (Edge X X)) (relation Seed (X)) (relation P (X)) (relation Q (X)) (relation R (X))\n(function Cost (X) i64 :merge (min old new)) (ruleset a) (ruleset b) (ruleset c) (ruleset barrier)\n(rule ((Edge x y) (P x)) ((Q y)) :ruleset a)\n(rule ((Q x)) ((R x)) :ruleset b)\n(rule ((Seed x)) ((P x)) :ruleset c)\n(rule ((Seed x)) ((set (Cost x) 1)) :ruleset barrier)\n(Seed (N 0)) (Edge (N 0) (N 1))\n(run-schedule (repeat 2 (run a) (run b) (run c) (run barrier))) (check (R (N 1)))')
 b=pair('escaping-result-fallback','(datatype X (N i64) (Tag X)) (function Chosen () X :merge new) (ruleset make) (ruleset consume)\n(rule ((= x (N n))) ((Tag x)) :ruleset make)\n(rule ((= y (Tag x))) ((set (Chosen) y)) :ruleset consume)\n(N 0) (run-schedule (saturate (run make)) (run consume)) (check (= (Chosen) (Tag (N 0))))')
 assert not b['scheduled_matrix']['calls']
 # 28 distinct synthetic stage types in one program, all on one shared fixed carrier.
 source='(datatype X (N i64))\n'+''.join(f'(relation P{i} (X)) (ruleset s{i})\n' for i in range(28))
 source+='(rule ((= x (N n))) ((P0 x)) :ruleset s0)\n'
 source+=''.join(f'(rule ((P{i-1} x)) ((P{i} x)) :ruleset s{i})\n' for i in range(1,28))
 source+=''.join(f'(N {i})\n' for i in range(256))
 source+='(run-schedule (repeat 5 '+''.join(f'(run s{i}) ' for i in range(28))+')) (check (P27 (N 255)))'
 b=pair('twenty-eight-stages',source);assert max(len(c['rulesets']) for c in b['scheduled_matrix']['calls'])==28
 f=out/'capacity-rejection.egg';f.write_text(BASE+chain(3)+'(run-schedule (saturate (run a) (run b)))')
 rollback=out/'capacity-transaction'
 if not (rollback/'verified.json').exists():
  result=rejected_transaction(build,f,rollback,5,capacity=10,scheduled=True)
  (rollback/'verified.json').write_text(json.dumps(result))
 results['capacity-transaction']=json.loads((rollback/'verified.json').read_text())
 (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');return results
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();print('passed',len(tests(a.build,a.output)))

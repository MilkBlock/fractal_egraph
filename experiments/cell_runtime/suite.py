"""Frozen Pass-3 correctness survey, with independently cached native controls.

Not a hardware/profile benchmark and not permission to ignore Smaller differences.
"""
import argparse,collections,hashlib,json,subprocess,sys,time,re,platform,os
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT))
from eggfuse.engine.cell_runtime import prepare,execute,binary
from eggfuse.engine.cpu import binary as native_binary,compare_native
from eggfuse.ir.cell_runtime import build
from experiments.cell_runtime.audit import check as audit_bindings

def fingerprint(path):
 return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def timed(program, engine, output):
 """One clean process; parsing/export/RSS separate from inference, no audit."""
 if any(k in os.environ for k in ('CELL_CPU_PROFILE','CELL_CPU_PROGRESS')):raise ValueError('diagnostic mode is not a clean benchmark')
 output.parent.mkdir(parents=True,exist_ok=True)
 log=output.with_suffix('.log');start=time.perf_counter()
 cmd=[str(engine.resolve()),'--program',str(program.resolve()),'--output',str(output.resolve())]
 if sys.platform=='darwin':cmd=['/usr/bin/time','-l']+cmd
 with log.open('w') as f:subprocess.run(cmd,stdout=f,stderr=subprocess.STDOUT,check=True)
 elapsed=time.perf_counter()-start;report=json.loads((output/'report.json').read_text())
 rss=re.search(r'(\d+)\s+maximum resident set size',log.read_text())
 packet={'program_sha256':fingerprint(program),'binary_sha256':fingerprint(engine),'machine':platform.node(),'wall_seconds':elapsed,'peak_rss_bytes':int(rss[1]) if rss else None,'inference_seconds':report['inference_seconds'],'setup_seconds':report['setup_seconds'],'audit':False,'configuration':{k:v for k,v in os.environ.items() if k.startswith('CELL_CPU_')}}
 (output/'measurement.json').write_text(json.dumps(packet,indent=2)+'\n')
 return packet,report

def benchmark(args):
 """Frozen control reuse for the standalone backend (not the older CPU worker)."""
 engine=args.engine or binary();args.output.mkdir(parents=True,exist_ok=False);summary={}
 for name in args.cases:
  control=args.baseline/name;program=control/'program.json'
  before=json.loads((control/'runtime/measurement.json').read_text())
  assert before['program_sha256']==fingerprint(program) and before['machine']==platform.node() and before['audit'] is False
  if args.audit_baseline_engine:
   assert fingerprint(args.audit_baseline_engine)==before['binary_sha256']
   data=json.loads(program.read_text());d=args.output/name;d.mkdir()
   execute(data,d/'control',args.audit_baseline_engine,audit=True,environment=before.get('configuration',{}))
   execute(data,d/'candidate',engine,audit=True)
   for file in ('state.json','stages.json','binding-audit.json'):
    assert json.loads((d/'control/runtime'/file).read_text())==json.loads((d/'candidate/runtime'/file).read_text()),(name,file)
   summary[name]={'all_stage_states_and_bindings_exact':True,'independent_lhs_audit':audit_bindings(data,d/'candidate/runtime'),'scope':'correctness with audit overhead; NOT a timing run'}
   (args.output/'results.json').write_text(json.dumps(summary,indent=2)+'\n');print(name,summary[name],flush=True)
   continue
  measured,report=timed(program,engine,args.output/name)
  old=json.loads((control/'runtime/report.json').read_text())
  assert json.loads((control/'runtime/state.json').read_text())==json.loads((args.output/name/'state.json').read_text()),name+' full state differs'
  keys=('ordinal','ruleset','path','iterations','epoch','logical_matches','changed','live_cells')
  assert [[s[k] for k in keys] for s in old['steps']]==[[s[k] for k in keys] for s in report['steps']],name+' Run semantics differ'
  for key in ('bindings','constructor_requests','logical_facts_created','function_updates','merge_calls','unions'):
   assert old['stats'][key]==report['stats'][key],(name,key)
  phases=collections.Counter()
  for s in report['steps']:phases.update(s.get('phases',{}))
  summary[name]={'baseline':before,'optimized':measured,'speedup':before['inference_seconds']/measured['inference_seconds'],'runs':len(report['steps']),'logical_matches':report['stats']['bindings'],'baseline_candidates':old['stats']['residual_candidates'],'optimized_candidates':report['stats']['residual_candidates'],'sieve':report.get('sieve'),'join_refinements':report.get('join_refinements'),'baseline_decoded_rows':old['stats']['decoded_rows'],'optimized_decoded_rows':report['stats']['decoded_rows'],'phase_seconds':dict(phases),'exact_state_and_schedule':True,'native_equivalence':'not asserted; separate native survey retains known discrepancies'}
  print(name,summary[name]['speedup'],measured['inference_seconds'],dict(phases),flush=True)
  (args.output/'results.json').write_text(json.dumps(summary,indent=2)+'\n')

def freeze(args):
 engine=args.engine or binary();args.output.mkdir(parents=True,exist_ok=False)
 suite=ROOT/'experiments/profile_suite';spec=json.loads((suite/'suite.json').read_text())
 for name in args.cases:
  case=next(c for c in spec['cases'] if c['name']==name)
  source=b''.join((suite/part).read_bytes() for part in case['parts']);assert hashlib.sha256(source).hexdigest()==case['sha256']
  d=args.output/name;d.mkdir();src=d/'source.egg';src.write_bytes(source)
  program=d/'program.json';program.write_text(json.dumps(build(prepare(src))))
  measured,_=timed(program,engine,d/'runtime');print(name,measured,flush=True)

def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--controls',type=Path,default=ROOT/'out/cell-runtime-native-controls');p.add_argument('--cases',nargs='+',default=['fact','loopfact','sum-bits']);mode=p.add_mutually_exclusive_group();mode.add_argument('--baseline',type=Path,help='Clean benchmark against frozen controls, without full audit');mode.add_argument('--freeze',action='store_true',help='Save clean controls for the current standalone CPU binary');p.add_argument('--engine',type=Path);p.add_argument('--audit-baseline-engine',type=Path,help='Separate full per-stage audit against this frozen binary, no timing');a=p.parse_args()
 if a.audit_baseline_engine and not a.baseline:p.error('--audit-baseline-engine requires --baseline')
 if a.freeze:return freeze(a)
 if a.baseline:return benchmark(a)
 a.output.mkdir(parents=True,exist_ok=False);a.controls.mkdir(parents=True,exist_ok=True)
 engine=binary();native=native_binary();native_sha=hashlib.sha256(native.read_bytes()).hexdigest()
 suite=ROOT/'experiments/profile_suite';spec=json.loads((suite/'suite.json').read_text());summary={}
 for name in a.cases:
  case=next(c for c in spec['cases'] if c['name']==name);source=b''.join((suite/part).read_bytes() for part in case['parts']);assert hashlib.sha256(source).hexdigest()==case['sha256']
  d=a.output/name;d.mkdir();src=d/'source.egg';src.write_bytes(source);program=build(prepare(src));report=execute(program,d/'cell',engine,audit=True)
  audited=audit_bindings(program,d/'cell/runtime')
  # The cache includes both source bytes and actual native binary, not a label.
  cache=a.controls/(name+'-'+hashlib.sha256((case['sha256']+native_sha).encode()).hexdigest()[:20])
  reused=(cache/'native/state.json').exists()
  if not reused:
   cache.mkdir(exist_ok=True)
   with (cache/'native.log').open('w') as f:subprocess.run([str(native),'--source',str(src.resolve()),'--mode','native','--output',str((cache/'native').resolve())],stdout=f,stderr=subprocess.STDOUT,check=True)
  actual=json.loads((d/'cell/runtime/state.json').read_text());reference=json.loads((cache/'native/state.json').read_text())
  actual['rows']=[r for r in actual['rows'] if r['table'] not in program['source_core'].get('assertion_tables',[])]
  verification=None;failure=None
  try:verification=compare_native(program['source_core'],actual,reference)
  except AssertionError as e:failure=str(e)
  ac=collections.Counter(r['table'] for r in actual['rows']);bc=collections.Counter(r['table'] for r in reference['rows'])
  differences={t:{'cell':ac[t],'native':bc[t]} for t in sorted(ac|bc) if ac[t]!=bc[t]}
  diagnostic=None
  if failure:
   left={**actual,'rows':[r for r in actual['rows'] if r['table']!='Smaller']};right={**reference,'rows':[r for r in reference['rows'] if r['table']!='Smaller']}
   try:diagnostic={'excluded':['Smaller'],'result':compare_native(program['source_core'],left,right),'scope':'diagnostic projection only; NOT full correctness acceptance'}
   except AssertionError as e:diagnostic={'error':str(e),'scope':'diagnostic projection also failed'}
  execute(program,d/'replay',engine)
  same=json.loads((d/'cell/runtime/state.json').read_text())==json.loads((d/'replay/runtime/state.json').read_text())
  summary[name]={'source_sha256':case['sha256'],'native_binary_sha256':native_sha,'native_control_reused':reused,'schedule_completed':report['status']=='complete','binding_audit':audited,'native_full_comparison':verification,'native_full_failure':failure,'row_count_differences':differences,'diagnostic_projection':diagnostic,'fresh_process_state_reproducible':same,'runs':len(report['steps']),'cell_report':{k:v for k,v in report.items() if k!='steps'},'scope':'CPU Cell executor correctness survey with full audit overhead; no GPU acceleration result'}
  (d/'result.json').write_text(json.dumps(summary[name],indent=2)+'\n');print(name,'runs',len(report['steps']),'full equality',verification is not None,'repeat',same,'differences',differences,flush=True)
 (a.output/'results.json').write_text(json.dumps(summary,indent=2)+'\n')
if __name__=='__main__':main()

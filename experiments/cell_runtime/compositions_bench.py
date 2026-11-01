"""Observed multi-rule recipes: official compiler pipeline, paired metrics/gate.

No raw source_core execution path. Existing clean timing can be followed by an
audit-only run of the identical binary. The merge gate never uses hit rate.
"""
import argparse,collections,json,os,platform,subprocess,time
from pathlib import Path
from contextlib import contextmanager
from eggfuse.engine.cell_runtime import load_pipeline_artifact,binary
from experiments.cell_runtime.reference_views_bench import reemit
from experiments.cell_runtime.suite import timed,fingerprint

def read(p):return json.loads(p.read_text())
@contextmanager
def mode(enabled):
 settings={'CELL_CPU_REFERENCE_VIEWS':'1','CELL_CPU_MARKER_FIELDS':'copy','CELL_CPU_ABSORB':'1','CELL_CPU_MULTISTAGE':'1','CELL_CPU_COMPOSITIONS':str(int(enabled))}
 before={k:os.environ.get(k) for k in settings}
 try:os.environ.update(settings);yield
 finally:
  for k,v in before.items():
   if v is None:os.environ.pop(k,None)
   else:os.environ[k]=v

def audit(path,engine,out):
 with mode(True):
  r=subprocess.run([str(engine),'--program',str(path),'--verify-compositions'],text=True,capture_output=True)
 (out/'audit.log').write_text(r.stdout+r.stderr)
 if r.returncode:raise RuntimeError(r.stderr)
 return json.loads(r.stdout)

def metrics(report):
 phases=collections.Counter()
 for step in report['steps']:phases.update(step['phases'])
 return {'phase_seconds':dict(phases),'key_lookups':report['key_lookup_calls'],
         'candidate_visits':report['stats']['residual_candidates'],
         'duplicate_constructor_requests':report['stats']['constructor_reused'],
         'constructor_requests':report['stats']['constructor_requests'],
         'union_requests':report['stats']['union_requests'],'effective_unions':report['stats']['unions'],
         'merge_calls':report['stats']['merge_calls'],'repair_visits':report['stats']['repaired_dependencies'],
         'repair_collisions':report['stats']['repair_collisions'],
         'logical_bindings':report['stats']['bindings'],
         'live_cells':report['live_cells'],'logical_records':report['virtual_constructor_facts']+report['function_ports'],
         'compositions':report['compositions'],'reference_views':report['reference_views']}

def merge_gate(result,guards=()):
 primary=result['ratios']['inference']<=.9 and result['ratios']['rss']<=1.1 and any(v is not None and v>=.2 for v in result['ratios']['work_reductions'].values())
 verified=bool(result.get('verification',{})) and result['verification'].get('all_stage_graphs_and_bindings_equal',False)
 source=result['provenance']['source_sha256'];identities=[g['provenance']['source_sha256'] for g in guards]
 binary=result['lanes']['on']['measurement']['binary_sha256']
 guard_ok=len(guards)==2 and len(set([source]+identities))==3 and all(
  (g.get('verification') or {}).get('all_stage_graphs_and_bindings_equal',False)
  and g['ratios']['inference']<=1.05 and g['ratios']['rss']<=1.1
  and all(x['measurement']['binary_sha256']==binary for x in g['lanes'].values()) for g in guards)
 return {'primary_performance_passed':primary,'full_audit_passed':verified,'guard_cases_passed':guard_ok,
         'merge_allowed':primary and verified and guard_ok,
         'guard_cases_status':'passed' if guard_ok else ('not run: primary gate already failed' if not primary else 'two distinct, audited guard cases required'),
         'reason':'eligible for review' if primary and verified and guard_ok else ('primary performance threshold not met' if not primary else 'correctness/guard gates incomplete')}

def run(args):
 out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
 engine=(args.engine or binary()).resolve()
 if args.audit_existing:
  old=read(args.audit_existing/'results.json');path=Path(old['program'])
  for lane in old['lanes'].values():
   assert lane['measurement']['binary_sha256']==fingerprint(engine)
   assert lane['measurement']['program_sha256']==fingerprint(path)
  if args.reuse_audit:
   saved=read(args.reuse_audit)
   if saved.get('program_sha256')!=fingerprint(path) or saved.get('binary_sha256')!=fingerprint(engine):raise ValueError('audit fingerprint differs')
   verification=saved['verification']
   if not verification.get('all_stage_graphs_and_bindings_equal'):raise ValueError('audit did not pass')
  else:verification=audit(path,engine,out)
  old['verification']=verification;old['timings_reused_from']=str(args.audit_existing.resolve()/'results.json')
  old['program_sha256']=fingerprint(path);old['binary_sha256']=fingerprint(engine)
  old['gate']=merge_gate(old,[read(p/'results.json') for p in args.guard_results])
  (out/'results.json').write_text(json.dumps(old,indent=2))
  print(json.dumps(verification,indent=2));return
 if args.compiled:
  path,p,provenance=load_pipeline_artifact(args.compiled.resolve());compile_seconds=0.0
  if not p.get('compositions',{}).get('edges'):raise ValueError('compiled artifact has no multi-rule recipes')
 else:
  path,p,provenance=load_pipeline_artifact(args.pipeline.resolve())
  if args.observed_run:
   observed=args.observed_run.resolve();m=read(observed/'measurement.json')
   if m['program_sha256']!=fingerprint(path):raise ValueError('observations came from another compiler artifact')
   report=read(observed/'report.json')
  else:
   # Pilot only selects observed transitions. Its timing is not a comparison lane.
   with mode(False):_,report=timed(path,engine,out/'pilot')
   observed=out/'pilot'
  if report['status']!='complete':raise ValueError('incomplete observations')
  evidence={'recipe_sha256':p['recipe']['sha256'],'pairs':report['multi_stage']['executed_rule_pairs'],
            'origin':{'report':str(observed/'report.json'),'report_sha256':fingerprint(observed/'report.json'),'program_sha256':fingerprint(path)}}
  evidence_file=out/'evidence.json';evidence_file.write_text(json.dumps(evidence,indent=2))
  start=time.perf_counter();path,p,provenance=reemit(args.pipeline.resolve(),out/'compile',evidence_file);compile_seconds=time.perf_counter()-start
 verification=None
 if not args.timing_only:verification=audit(path,engine,out)
 lanes={};state=None;schedule=None
 for enabled in (False,True):
  name='on' if enabled else 'off'
  with mode(enabled):measurement,report=timed(path,engine,out/name)
  current=read(out/name/'state.json');current['rows'].sort(key=lambda r:json.dumps(r,sort_keys=True))
  control=[{k:s[k] for k in ('ordinal','ruleset','path','iterations','epoch','logical_matches','changed')} for s in report['steps']]
  if state is None:state=current;schedule=control
  elif state!=current or schedule!=control:raise ValueError('endpoint or source Run sequence differs')
  lanes[name]={'measurement':measurement,**metrics(report)}
 a,b=lanes['off'],lanes['on'];ratio=b['measurement']['inference_seconds']/a['measurement']['inference_seconds']
 reductions={k:1-b[k]/a[k] if a[k] else None for k in ('key_lookups','candidate_visits','duplicate_constructor_requests','union_requests','merge_calls','repair_visits')}
 rss=b['measurement']['peak_rss_bytes']/a['measurement']['peak_rss_bytes']
 result={'schema':'eggfuse-composition-bench/v1','scope':'same-binary standalone Cell CPU, identical source_sk_mk and schedule; not pristine egglog',
         'program':str(path),'provenance':provenance,'verification':verification,'lanes':lanes,
         'compile_seconds':compile_seconds,'library_bytes':(path.parent/'composition-program.json').stat().st_size,
         'compiled':{'programs':len(p['compositions']['programs']),'edges':len(p['compositions']['edges']),'rejected':p['compositions']['rejected']},
         'ratios':{'inference':ratio,'rss':rss,'work_reductions':reductions},
         }
 result['gate']=merge_gate(result,[read(p/'results.json') for p in args.guard_results])
 (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps({k:result[k] for k in ('compiled','ratios','gate')},indent=2))

def main():
 p=argparse.ArgumentParser();s=p.add_mutually_exclusive_group(required=True);s.add_argument('--pipeline',type=Path);s.add_argument('--compiled',type=Path);s.add_argument('--audit-existing',type=Path)
 p.add_argument('--observed-run',type=Path);p.add_argument('--output',type=Path,required=True);p.add_argument('--engine',type=Path);p.add_argument('--timing-only',action='store_true');p.add_argument('--reuse-audit',type=Path)
 p.add_argument('--guard-results',type=Path,nargs='*',default=[],help='Two distinct audited workloads using the same binary; eligibility only, never invokes Git')
 args=p.parse_args()
 if args.reuse_audit and not args.audit_existing:p.error('reuse-audit requires audit-existing')
 run(args)
if __name__=='__main__':main()

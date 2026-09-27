#!/usr/bin/env python3
"""Controlled RHS-skip experiment. Verification is outside timed processes."""
import argparse,hashlib,json,os,statistics,subprocess
from pathlib import Path
from math_virtual import ROOT,measure
from math_graph_equal import verify
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',required=True);p.add_argument('--rounds',type=int,default=8);p.add_argument('--runs',type=int,default=3);a=p.parse_args()
if min(a.rounds,a.runs)<1:p.error('positive rounds/runs required')
out=(ROOT/a.output).resolve();out.mkdir(parents=True,exist_ok=False)
source=(ROOT/'experiments/math_mini3/source.egg').read_text();assert source.count('(run 3)')==1
src=out/'source.egg';src.write_text(source.replace('(run 3)',f'(run {a.rounds})'))
lib=ROOT/'experiments/math_mini3/adaptive_runtime/bake_lib_round2.json';binary=ROOT/'target/release/examples/adaptive_math'
report={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_sha256':hashlib.sha256(src.read_bytes()).hexdigest(),'library_sha256':hashlib.sha256(lib.read_bytes()).hexdigest(),'parent_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'rayon_threads':os.environ.get('RAYON_NUM_THREADS','automatic'),'runs':[],'verification':{},'notes':['Same fixed library and eight-round source by default. Normal is the untraced adaptive runtime, not pristine egglog.', 'Audit vs skip isolates the RHS predicate under dependency instrumentation. This is not a production-speed matcher implementation.', 'Graph verification runs are separate and excluded from medians. The filter executes after the Join, before RHS instructions.', 'Writes are trace event counts, not counts of surviving distinct enodes. Skip may omit private notification packets.']}
def mode_env(mode,verify_graph):
 os.environ['EGG_LAYOUT_MIN_TEMPLATE_REUSE']='2'
 os.environ['EGG_LAYOUT_AUDIT_ENCLOSURES']='0' if mode=='normal' else '1'
 os.environ['EGG_LAYOUT_SKIP_INTERNAL_RHS']='1' if mode=='skip' else '0'
 os.environ['EGG_LAYOUT_AUDIT_VERIFY']='1' if verify_graph else '0'
for mode in ['normal','audit','skip']:
 mode_env(mode,True);dest=out/f'verify-{mode}'
 measure([binary,'frozen',src,dest,lib],out/f'verify-{mode}-process')
 if mode!='normal':report['verification'][mode]=verify(out/'verify-normal/verification-graph.json',dest/'verification-graph.json')
for i in range(a.runs):
 modes=['normal','audit','skip'];modes=modes[i%3:]+modes[:i%3]
 for mode in modes:
  mode_env(mode,False);dest=out/f'{mode}-{i}'
  r=measure([binary,'frozen',src,dest,lib],out/f'{mode}-{i}-process');r.update(mode=mode,repeat=i);report['runs'].append(r)
  (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
report['medians']={m:{'driver_ms':statistics.median(r['result']['seconds']*1000 for r in report['runs'] if r['mode']==m),'wall_seconds':statistics.median(r['process_wall_seconds'] for r in report['runs'] if r['mode']==m),'cpu_seconds':statistics.median(r['user_seconds']+r['system_seconds'] for r in report['runs'] if r['mode']==m),'peak_rss_bytes':statistics.median(r['peak_rss_bytes'] for r in report['runs'] if r['mode']==m)} for m in ['normal','audit','skip']}
for m in ['audit','skip']:
 counters=[]
 for run in report['runs']:
  if run['mode']!=m:continue
  samples=[x['enclosure_audit'] for x in run['result']['samples']]
  counters.append({'candidates':sum(x['candidate_matches'] for x in samples),'skipped_rhs':sum(x['rhs_skipped'] for x in samples),'traced_nonredundant_unions':sum(x['nonredundant_unions'] for x in samples),'native_union_merges_total':sum(x['native_union_merges_total'] for x in samples),**{k:sum(x['math_write_events'].get(k,0) for x in samples) for k in ['Inserted','Updated','Deduplicated','Unsupported']}})
 report.setdefault('counters_median',{})[m]={k:statistics.median(r[k] for r in counters) for k in counters[0]}
(out/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k] for k in ['verification','medians','counters_median']},indent=2))

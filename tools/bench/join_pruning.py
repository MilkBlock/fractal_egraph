#!/usr/bin/env python3
"""Untraced native Join ablation with exact graph verification outside timing."""
import argparse, hashlib, json, os, statistics
from math_virtual import ROOT, measure
from math_graph_equal import verify
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--output',required=True);p.add_argument('--runs',type=int,default=3)
p.add_argument('--rounds',type=int,default=8);a=p.parse_args()
if min(a.runs,a.rounds)<1:p.error('positive runs and rounds required')
out=ROOT/a.output;out.mkdir(parents=True,exist_ok=False)
src=out/'source.egg';source=(ROOT/'experiments/math_mini3/source.egg').read_text()
assert source.count('(run 3)')==1
src.write_text(source.replace('(run 3)',f'(run {a.rounds})'))
lib=ROOT/'experiments/math_mini3/adaptive_runtime/bake_lib_round2.json'
binary=ROOT/'target/release/examples/adaptive_math'
r={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_sha256':hashlib.sha256(src.read_bytes()).hexdigest(),'library_sha256':hashlib.sha256(lib.read_bytes()).hexdigest(),'threads':os.environ.get('RAYON_NUM_THREADS','auto'),'runs':[], 'scope':'Same frozen library; normal and prune both untraced, with all certificate preparation included. Not a pristine egglog comparison.'}
def run(mode,label,verification=False):
 os.environ.update(EGG_LAYOUT_MIN_TEMPLATE_REUSE='2',EGG_LAYOUT_AUDIT_ENCLOSURES='0',EGG_LAYOUT_SKIP_INTERNAL_RHS='0',EGG_LAYOUT_PRUNE_INTERNAL_JOIN=str(int(mode=='prune')),EGG_LAYOUT_AUDIT_VERIFY=str(int(verification)))
 return measure([binary,'frozen',src,out/label,lib],out/(label+'-process'))
for mode in ['normal','prune']:run(mode,'verify-'+mode,True)
r['verification']=verify(out/'verify-normal/verification-graph.json',out/'verify-prune/verification-graph.json')
for i in range(a.runs):
 for mode in (['normal','prune'] if i%2==0 else ['prune','normal']):
  x=run(mode,f'{mode}-{i}');x.update(mode=mode,repeat=i)
  full=x['result']
  x['result']={k:full[k] for k in ['seconds','execution_seconds','matches','final']}
  x['result']['samples']=[{k:v for k,v in sample.items() if k in ['round','source_matches','macro_matches','cumulative_matches','enclosure_audit']} for sample in full['samples']]
  r['runs'].append(x)
  (out/'report.json').write_text(json.dumps(r,indent=2)+'\n')
r['medians']={m:{'driver_ms':statistics.median(x['result']['seconds']*1000 for x in r['runs'] if x['mode']==m),'execution_ms':statistics.median(x['result']['execution_seconds']*1000 for x in r['runs'] if x['mode']==m),'matches':statistics.median(x['result']['matches'] for x in r['runs'] if x['mode']==m),'peak_rss_bytes':statistics.median(x['peak_rss_bytes'] for x in r['runs'] if x['mode']==m)} for m in ['normal','prune']}
(out/'report.json').write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({'verification':r['verification'],'medians':r['medians']},indent=2))

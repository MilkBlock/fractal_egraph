#!/usr/bin/env python3
"""Equal-result post-saturation packing versus the verified pristine binary."""
import argparse, hashlib, json, os, statistics, subprocess, sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/bench'))
from math_virtual import measure
from math_graph_equal import verify

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',required=True);p.add_argument('--runs',type=int,default=3);p.add_argument('--require-net-rss',action='store_true');a=p.parse_args()
 if a.runs<1:p.error('positive runs required')
 os.chdir(ROOT);out=Path(a.output).resolve();out.mkdir(parents=True,exist_ok=False)
 pristine=ROOT/'out/native-compare/bin/pristine';current=ROOT/'target/release/examples/post_pack_bench'
 historical=json.loads((ROOT/'experiments/native_compare/results/final.json').read_text())
 sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
 provenance=ROOT/'out/native-compare/build.json'
 if provenance.exists():
  build=json.loads(provenance.read_text());assert build['baseline_commit']==subprocess.check_output(['git','rev-parse','egglog-baseline^{}'],text=True).strip() and build['baseline_source_clean'] and build['registry_versions_pinned'];assert sha(pristine)==build['binary_sha256']['pristine']
 else:assert sha(pristine)==historical['binary_sha256']['pristine'],'rebuild/re-verify pristine provenance before benchmarking'
 os.environ.pop('RAYON_NUM_THREADS',None)
 source=ROOT/'egglog/tests/math-microbenchmark.egg'
 variants={'pristine':(None,None),'native':(None,None),'match-full':('full','match'),'pack-full':('full','pack'),'match-r6':('r6','match'),'pack-r6':('r6','pack')}
 report={'schema':'memory-fix/v1','source_sha256':sha(source),'baseline_commit':subprocess.check_output(['git','rev-parse','egglog-baseline^{}'],text=True).strip(),'base_commit':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'dirty':True,'threads':'auto','allocator':'System','binary_sha256':{'pristine':sha(pristine),'current':sha(current)},'libraries':{n:{'path':f'out/math-lib-{n}/library.json','bytes':(ROOT/f'out/math-lib-{n}/library.json').stat().st_size,'sha256':sha(ROOT/f'out/math-lib-{n}/library.json')} for n in ['full','r6']},'verification':{},'runs':[],'notes':['Actual native execution of unchanged 11-round source; frozen-closure rewriting is not enabled.','Pristine binary is hash-verified against the recorded clean immutable-tag build; both System allocator and ThinLTO/codegen-units=1.','Current harness mirrors native_compare, adding optional measured library load/query/packing. No allocator instrumentation or graph scan during timed runs.','Separate exports verify all constructor facts and equivalence classes modulo IDs.','Pack times include discovery, loading and packing, not offline Bake. Native baseline has no library.','No candidate budgets or early exit: external process guard only (90s / 4GiB).','The richer library is from a previously interrupted capture; not a complete discovered library.']}
 def run(name,suffix,export=False):
  lib,mode=variants[name];os.environ.pop('MEMORY_FIX_LIBRARY',None);os.environ.pop('MEMORY_FIX_MODE',None)
  if lib:os.environ.update(MEMORY_FIX_LIBRARY=str(ROOT/f'out/math-lib-{lib}/library.json'),MEMORY_FIX_MODE=mode)
  cmd=[pristine if name=='pristine' else current,source]
  if export:cmd.append(out/f'{name}-graph.json')
  return measure(cmd,out/f'{name}-{suffix}',timeout=90)
 for name in ['pristine','native','pack-full','pack-r6']:
  run(name,'verify',True)
  report['verification'][name]=verify(out/'pristine-graph.json',out/f'{name}-graph.json')
  print('verified',name,report['verification'][name],flush=True)
 for i in range(a.runs):
  names=list(variants)
  if i%2:names.reverse()
  for name in names:
   r=run(name,str(i));r.update(variant=name,repetition=i);report['runs'].append(r)
   (out/'results.json').write_text(json.dumps(report,indent=2)+'\n')
 report['medians']={n:{'seconds':statistics.median(r['result']['seconds'] for r in report['runs'] if r['variant']==n),'through_drop_seconds':statistics.median(r['result']['through_drop_seconds'] for r in report['runs'] if r['variant']==n),'process_wall_seconds':statistics.median(r['process_wall_seconds'] for r in report['runs'] if r['variant']==n),'peak_rss_bytes':statistics.median(r['peak_rss_bytes'] for r in report['runs'] if r['variant']==n)} for n in variants}
 report['net_rss_acceptance']={n:report['medians'][n]['peak_rss_bytes']<report['medians']['pristine']['peak_rss_bytes'] for n in ['pack-full','pack-r6']}
 (out/'results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report['medians'],indent=2))
 if a.require_net_rss and not all(report['net_rss_acceptance'].values()):raise SystemExit('FAILED: packing has not demonstrated lower peak RSS than pristine egglog')
if __name__=='__main__':main()

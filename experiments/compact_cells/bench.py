#!/usr/bin/env python3
"""Actual native compact-cell storage: equal-graph pristine/row/cell comparison."""
import argparse,hashlib,json,os,statistics,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/bench'))
from math_virtual import measure
from math_graph_equal import verify
p=argparse.ArgumentParser();p.add_argument('--output',required=True);p.add_argument('--runs',type=int,default=1);a=p.parse_args()
os.chdir(ROOT);out=Path(a.output).resolve();out.mkdir(parents=True,exist_ok=False)
source=ROOT/'egglog/tests/math-microbenchmark.egg';library=ROOT/'out/math-lib-full/library.json'
pristine=ROOT/'out/native-compare/bin/pristine';binary=ROOT/'target/release/examples/post_pack_bench'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
provenance=ROOT/'out/native-compare/build.json'
if provenance.exists():
 b=json.loads(provenance.read_text());assert b['baseline_commit']==subprocess.check_output(['git','rev-parse','egglog-baseline^{}'],text=True).strip() and b['baseline_source_clean'] and b['registry_versions_pinned'];assert sha(pristine)==b['binary_sha256']['pristine']
else:assert sha(pristine)==json.loads((ROOT/'experiments/native_compare/results/final.json').read_text())['binary_sha256']['pristine']
os.environ.pop('RAYON_NUM_THREADS',None)
report={'scope':'native unchanged Math source, existing committed facts; no accelerated RHS or tracing','base_commit':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'dirty':True,'pristine_sha256':sha(pristine),'current_sha256':sha(binary),'source_sha256':sha(source),'library_sha256':sha(library),'library_bytes':library.stat().st_size,'runs':[],'verification':{}}
def run(mode,suffix,check=False):
 for k in ['MEMORY_FIX_LIBRARY','MEMORY_FIX_MODE','CELL_STORAGE_REPORT']:os.environ.pop(k,None)
 if mode in ['row','cell']:os.environ.update(MEMORY_FIX_LIBRARY=str(library),MEMORY_FIX_MODE='cell' if mode=='cell' else 'pack')
 if check and mode=='cell':os.environ['CELL_STORAGE_REPORT']=str(out/'cell-storage.json')
 cmd=[pristine if mode=='pristine' else binary,source]
 if check:cmd.append(out/f'{mode}-graph.json')
 return measure(cmd,out/f'{mode}-{suffix}',timeout=90)
for mode in ['pristine','native','row','cell']:
 run(mode,'check',True);report['verification'][mode]=verify(out/'pristine-graph.json',out/f'{mode}-graph.json');print('verified',mode,flush=True)
for i in range(a.runs):
 for mode in (['pristine','native','row','cell'] if i%2==0 else ['cell','row','native','pristine']):
  r=run(mode,str(i));r.update(mode=mode,repetition=i);report['runs'].append(r)
  (out/'results.json').write_text(json.dumps(report,indent=2)+'\n')
report['medians']={m:{k:statistics.median(r['result'][k] for r in report['runs'] if r['mode']==m) for k in ['seconds','through_drop_seconds']} for m in ['pristine','native','row','cell']}
for m in report['medians']:report['medians'][m]['peak_rss_bytes']=statistics.median(r['peak_rss_bytes'] for r in report['runs'] if r['mode']==m)
report['cell_storage']=json.loads((out/'cell-storage.json').read_text())
report['net_peak_rss_pass']=report['medians']['cell']['peak_rss_bytes']<report['medians']['pristine']['peak_rss_bytes']
(out/'results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report['medians'],indent=2))

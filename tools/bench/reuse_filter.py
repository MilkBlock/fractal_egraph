#!/usr/bin/env python3
"""Fixed executable library: isolate storage admission from learning/scheduling."""
import argparse, json, os, statistics
from pathlib import Path
from math_virtual import ROOT, measure
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',required=True);p.add_argument('--library',required=True);p.add_argument('--full',action='store_true');a=p.parse_args()
out=(ROOT/a.output).resolve();out.mkdir(parents=True,exist_ok=False);lib=(ROOT/a.library).resolve();binary=ROOT/'target/release/examples/adaptive_math';report={'runs':[],'scope':'same fixed library and inference; only batch distinct-instance packing threshold changes'}
for repeat in range(3):
 for threshold in ([1,2,8][repeat:]+[1,2,8][:repeat]):
  os.environ['EGG_LAYOUT_MIN_TEMPLATE_REUSE']=str(threshold)
  r=measure([binary,'frozen',ROOT/'experiments/math_mini3/source.egg',out/f'run-{threshold}-{repeat}',lib],out/f'process-{threshold}-{repeat}');r.update(threshold=threshold,repeat=repeat);report['runs'].append(r)
reference=report['runs'][0]['result']
for r in report['runs']:
 assert [(s['matches'],s['graph']['enodes'],s['graph']['eclasses']) for s in r['result']['samples']]==[(s['matches'],s['graph']['enodes'],s['graph']['eclasses']) for s in reference['samples']]
report['medians']={str(t):{'driver_ms':statistics.median(r['result']['seconds']*1000 for r in report['runs'] if r['threshold']==t),'rss_bytes':statistics.median(r['peak_rss_bytes'] for r in report['runs'] if r['threshold']==t),'packing_ms':statistics.median(sum(s['packing_seconds'] for s in r['result']['samples'])*1000 for r in report['runs'] if r['threshold']==t)} for t in [1,2,8]}
if a.full:
 os.environ['EGG_LAYOUT_MIN_TEMPLATE_REUSE']='2'
 dest=out/'full-2'
 try:report['full']=measure([binary,'frozen',ROOT/'egglog/tests/math-microbenchmark.egg',dest,lib],out/'full-process')
 except RuntimeError as e:report['full']={'guard':str(e),'progress':json.loads((dest/'progress.json').read_text())}
(out/'measurements.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report['medians'],indent=2))

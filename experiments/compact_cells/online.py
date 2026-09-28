#!/usr/bin/env python3
"""Known-cell binding integration; frozen RHS recipes identical in both modes."""
import argparse,json,os,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT/'tools/bench'))
from math_virtual import measure
from math_graph_equal import verify
p=argparse.ArgumentParser();p.add_argument('--output',required=True);p.add_argument('--rounds',type=int,default=6);a=p.parse_args()
os.chdir(ROOT);out=Path(a.output).resolve();out.mkdir(parents=True,exist_ok=False)
source=(ROOT/'experiments/math_mini3/source.egg').read_text().replace('(run 3)',f'(run {a.rounds})');(out/'source.egg').write_text(source)
os.environ.update(RAYON_NUM_THREADS='1',EGG_LAYOUT_RUNTIME_SUMMARY='1')
results={'scope':'same frozen RHS recipes; not equal-round pristine comparison','rounds':a.rounds}
for mode in ['row','cell']:
 os.environ['EGG_LAYOUT_COMPACT_CELLS']=str(int(mode=='cell'));os.environ['EGG_LAYOUT_AUDIT_VERIFY']='1'
 check=measure(['target/release/examples/adaptive_math','frozen',out/'source.egg',out/f'{mode}-check','experiments/math_mini3/adaptive_runtime/bake_lib_round2.json'],out/f'{mode}-check-process',timeout=90)
 if mode=='cell':assert any(s['packing'].get('compact') is True for s in check['result']['samples']),'compact hook inactive'
 os.environ['EGG_LAYOUT_AUDIT_VERIFY']='0'
 results[mode]=measure(['target/release/examples/adaptive_math','frozen',out/'source.egg',out/mode,'experiments/math_mini3/adaptive_runtime/bake_lib_round2.json'],out/f'{mode}-process',timeout=90)
results['exact_graph']=verify(out/'row-check/verification-graph.json',out/'cell-check/verification-graph.json')
(out/'results.json').write_text(json.dumps(results,indent=2)+'\n')
print(results['exact_graph']);print({m:results[m]['result']['seconds'] for m in ['row','cell']})

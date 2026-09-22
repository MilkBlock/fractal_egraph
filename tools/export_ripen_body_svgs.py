#!/usr/bin/env python3
"""Export top-N ripen bodies using the real native ripen EGraph SVGs.

`native_ripen` writes `native-egraph.dot/svg` at the moment the actual EGraph is
exported. This tool ranks catalog states by the existing reuse-coverage evidence,
finds a trigger for each state, and copies that exact native graph into a
 supplement directory. It never reconstructs a Node/Row carrier. Older runs may
 predate native artifact export; missing cells are rerun from their recorded
 entry.egg into a temporary directory.
"""
from __future__ import annotations
import argparse, csv, json, os, shutil, subprocess, tempfile
from pathlib import Path


def parse_args():
 p=argparse.ArgumentParser()
 p.add_argument('run',type=Path,help='run directory with saturated-rule-composition/queue.json or catalog/')
 p.add_argument('out',type=Path,help='fresh output directory')
 p.add_argument('--top-n',type=int,default=3)
 p.add_argument('--max-rounds',type=int,default=8,
                help='budget for regenerating native artifacts from old cells')
 return p.parse_args()

def load(run):
 q=run/'saturated-rule-composition/queue.json'
 if q.is_file():
  queue=json.loads(q.read_text()); cat=queue['catalog']['catalog']
  cov=Path('out/ripen-paper-default-coverage/top100.json')
  cov=json.loads(cov.read_text()) if cov.is_file() else None
 else:
  folder=run/'catalog' if (run/'catalog/catalog.json').is_file() else run
  cat=json.loads((folder/'catalog.json').read_text()); queue=None; cov=None
 return cat,queue,cov

def choose(cat,cov,n):
 if cov: return [(int(r['state']),r) for r in cov['rows'][:n]]
 return [(int(g['saturated_rule_composition']),{'state':g['saturated_rule_composition']}) for g in cat.get('state_groups',[])[:n]]

def trigger_source(cat,state_id):
 for t in cat.get('triggers',[]):
  if t.get('saturated_rule_composition') == state_id:
   # catalog source is the cell directory, not the original source. This is
   # where native_ripen copied the real graph export.
   return Path(t['source']), t
 raise FileNotFoundError(f'no trigger for catalog state C{state_id}')

def native_binary():
 root=Path(__file__).resolve().parents[1]
 candidates=[]
 if os.environ.get('EGG_LAYOUT_BINARY'):
  candidates.append(Path(os.environ['EGG_LAYOUT_BINARY']))
 candidates.extend([root/'target/release/egg_layout',root/'target/debug/egg_layout'])
 for candidate in candidates:
  if candidate.is_file() and os.access(candidate,os.X_OK): return candidate
 raise FileNotFoundError('no egg_layout binary found; build target/release/egg_layout or set EGG_LAYOUT_BINARY')

def materialize_native(source,max_rounds):
 entry=source/'entry.egg'
 if not entry.is_file(): raise FileNotFoundError(f'{source} has no entry.egg to regenerate native artifacts')
 parent=Path(tempfile.mkdtemp(prefix='ripen-native-'))
 temp=parent/'run'
 try:
  proc=subprocess.run([str(native_binary()),'ripen',str(entry),str(temp),'--max-rounds',str(max_rounds)],text=True,capture_output=True,check=False)
  if proc.returncode != 0:
   raise RuntimeError(f'native ripen failed for {source}: {(proc.stderr or proc.stdout).strip()}')
  report=temp/'ripen.json'
  if report.is_file() and json.loads(report.read_text()).get('ripen',{}).get('state') not in (None,'Saturated'):
   raise RuntimeError(f'native ripen for {source} did not reach Saturated')
  dot=temp/'native-egraph.dot'; svg=temp/'native-egraph.svg'
  if not dot.is_file() or not svg.is_file(): raise RuntimeError(f'native ripen for {source} produced no native-egraph.dot/svg')
  return temp,dot,svg
 except Exception:
  shutil.rmtree(parent,ignore_errors=True); raise

def main():
 a=parse_args(); cat,queue,cov=load(a.run)
 if a.out.exists(): raise SystemExit(f'output already exists: {a.out}')
 a.out.mkdir(parents=True)
 entries=[]
 for rank,(sid,meta) in enumerate(choose(cat,cov,a.top_n),1):
  source,t=trigger_source(cat,sid)
  dot=source/'native-egraph.dot'; svg=source/'native-egraph.svg'
  regenerated=False; temp=None
  if not dot.is_file() or not svg.is_file():
   temp,dot,svg=materialize_native(source,a.max_rounds); regenerated=True
  stem=f'top-{rank:02d}-state-{sid:04d}'
  shutil.copy2(dot,a.out/f'{stem}.dot'); shutil.copy2(svg,a.out/f'{stem}.svg')
  if temp is not None: shutil.rmtree(temp.parent,ignore_errors=True)
  state_path=source/'saturated-rule-composition.json'
  if state_path.is_file(): shutil.copy2(state_path,a.out/f'{stem}.state.json')
  entries.append({'rank':rank,'state':sid,'count':meta.get('count'),'cumulative':meta.get('cumulative'),'coverage':meta.get('coverage'),'trigger_source':str(source.resolve()),'dot':f'{stem}.dot','svg':f'{stem}.svg','native_artifact':True,'regenerated_native_artifact':regenerated,'visualization_only':False})
 manifest={'schema':'ripen-body-native-svg/v3','run':str(a.run.resolve()),'catalog_schema':cat.get('schema'),'top_n':a.top_n,'max_rounds_for_regeneration':a.max_rounds,'source':'native-egraph.dot/svg written by the actual ripen EGraph','visualization_only':False,'entries':entries}
 (a.out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
 with (a.out/'index.csv').open('w',newline='') as f:
  w=csv.DictWriter(f,fieldnames=['rank','state','count','cumulative','coverage','dot','svg','regenerated_native_artifact']);w.writeheader();w.writerows([{k:e.get(k) for k in w.fieldnames} for e in entries])
 print(json.dumps({'output':str(a.out),'entries':len(entries),'native_artifacts':True},indent=2))
if __name__=='__main__': main()

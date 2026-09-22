#!/usr/bin/env python3
"""Export top-N ripen bodies through egglog's native DOT/SVG serializer.

The catalog state is a canonical body, not a replayable rule program. This
exporter creates a tiny typed carrier e-graph containing the same supported rows,
runs egglog's ``--to-dot --to-svg``, and records the exact state-to-source map.
The carrier is a visualization artifact; its metadata marks that it is not a
new execution or a tier0 replacement.
"""
from __future__ import annotations
import argparse, csv, json, re, subprocess
from pathlib import Path


def parse_args():
 p=argparse.ArgumentParser()
 p.add_argument('run',type=Path,help='run directory with saturated-rule-composition/queue.json or catalog/')
 p.add_argument('out',type=Path,help='fresh output directory')
 p.add_argument('--top-n',type=int,default=3)
 p.add_argument('--egglog',type=Path,default=Path('egglog/target/debug/egglog'))
 return p.parse_args()

def load(run):
 q=run/'saturated-rule-composition/queue.json'
 if q.is_file():
  queue=json.loads(q.read_text()); cat=queue['catalog']['catalog']; cov=json.loads((run/'../ripen-paper-default-coverage/top100.json').read_text()) if (run/'../ripen-paper-default-coverage/top100.json').is_file() else None
 else:
  folder=run/'catalog' if (run/'catalog/catalog.json').is_file() else run
  cat=json.loads((folder/'catalog.json').read_text()); queue=None; cov=None
 return run,cat,queue,cov

def top_states(cat,cov,top_n):
 if cov:
  return [(r['state'],r) for r in cov['rows'][:top_n]]
 groups=cat.get('state_groups',[])
 return [(g['saturated_rule_composition'],{'state':g['saturated_rule_composition'],'count':None,'cumulative':None,'coverage':None}) for g in groups[:top_n]]

def source_for(state,meta):
 # One carrier datatype per exported state. Each State value is an integer
 # vertex; Row stores operator/arguments/result. This preserves graph topology
 # while avoiding any claim that arbitrary source rules can be replayed here.
 ops=sorted({r['op'] for r in state['rows']})
 lines=['; generated visualization carrier; source execution is not replayed',
        '; state came from exact SaturatedRuleComposition catalog equivalence',
        '(datatype Body (Node i64) (Row i64 i64 i64 i64))']
 for i in range(len(state['values'])): lines.append(f'(Node {i})')
 for i,r in enumerate(state['rows']):
  args=r['args']+[r['result']]
  while len(args)<3: args.append(args[-1] if args else 0)
  lines.append(f'(Row {i} {args[0]} {args[1]} {args[2]})')
 lines.append('(run 1)')
 return '\n'.join(lines)+'\n'

def main():
 a=parse_args(); run,cat,queue,cov=load(a.run)
 if a.out.exists(): raise SystemExit(f'output already exists: {a.out}')
 a.out.mkdir(parents=True)
 states_dir=(a.run/'catalog/states') if (a.run/'catalog/states').is_dir() else (a.run/'catalog'/'states')
 chosen=top_states(cat,cov,a.top_n); manifest=[]
 for rank,(sid,meta) in enumerate(chosen,1):
  state=json.loads((states_dir/f'state-{sid:04}.json').read_text())
  base=a.out/f'top-{rank:02d}-state-{sid:04d}'; egg=base.with_suffix('.egg'); egg.write_text(source_for(state,meta))
  stem=str(egg.with_suffix(''))
  subprocess.run([str(a.egglog),'--to-dot','--to-svg','--max-functions','1000','--max-calls-per-function','10000',str(egg)],check=True)
  (a.out/f'top-{rank:02d}-state-{sid:04d}.state.json').write_text(json.dumps(state,indent=2)+'\n')
  manifest.append({'rank':rank,'state':sid,'count':meta.get('count'),'cumulative':meta.get('cumulative'),'coverage':meta.get('coverage'),'source':egg.name,'dot':base.with_suffix('.dot').name,'svg':base.with_suffix('.svg').name,'values':len(state['values']),'rows':len(state['rows']),'scope':state['scope'],'visualization_only':True})
 (a.out/'manifest.json').write_text(json.dumps({'schema':'ripen-body-native-svg/v1','run':str(a.run.resolve()),'catalog_schema':cat.get('schema'),'top_n':a.top_n,'source':'state files from exact saturated-rule-composition catalog','visualization_only':True,'entries':manifest},indent=2)+'\n')
 with (a.out/'index.csv').open('w',newline='') as f:
  w=csv.DictWriter(f,fieldnames=['rank','state','count','cumulative','coverage','values','rows','svg']);w.writeheader();w.writerows([{k:e.get(k) for k in w.fieldnames} for e in manifest])
 print(json.dumps({'output':str(a.out),'entries':len(manifest),'visualization_only':True},indent=2))
if __name__=='__main__':main()

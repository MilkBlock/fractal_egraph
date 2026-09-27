#!/usr/bin/env python3
"""Observe normalized boundary-to-cell transitions; never change native rules."""
import argparse,hashlib,json,os,collections,subprocess,shutil
from math_virtual import ROOT,measure
from math_graph_equal import verify
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--output',required=True);p.add_argument('--rounds',type=int,default=6)
a=p.parse_args()
if a.rounds<1:p.error('rounds must be positive')
out=ROOT/a.output;out.mkdir(parents=True,exist_ok=False)
binary=ROOT/'target/release/examples/adaptive_math'
fixtures=ROOT/'experiments/math_mini3/cell_evolution'
os.environ.update(EGG_LAYOUT_PRUNE_INTERNAL_JOIN='0',EGG_LAYOUT_SKIP_INTERNAL_RHS='0',EGG_LAYOUT_AUDIT_ENCLOSURES='0',EGG_LAYOUT_MIN_TEMPLATE_REUSE='1')
r={'schema':'cell-evolution-experiment/v1','parent_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'observer_source_sha256':hashlib.sha256((ROOT/'src/reuse_evolution.rs').read_bytes()).hexdigest(),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'threads':os.environ.get('RAYON_NUM_THREADS','auto'),'cases':{}}
for case in ['boundary','refinement','multi_parent','unrelated','math']:
 if case=='math':
  src=out/'math.egg';text=(ROOT/'experiments/math_mini3/source.egg').read_text();assert text.count('(run 3)')==1
  src.write_text(text.replace('(run 3)',f'(run {a.rounds})'))
  lib=ROOT/'experiments/math_mini3/adaptive_runtime/bake_lib_round2.json'
 else:
  src=fixtures/(case+'.egg');os.environ.update(EGG_LAYOUT_CELL_EVOLUTION='0',EGG_LAYOUT_AUDIT_VERIFY='0')
  measure([binary,'adaptive',fixtures/(case+'.train.egg'),out/(case+'-training')],out/(case+'-training-process'))
  lib=out/(case+'-training')/'bake_lib_round1.json'
 for mode in ['control','observe']:
  os.environ.update(EGG_LAYOUT_CELL_EVOLUTION=str(int(mode=='observe')),EGG_LAYOUT_AUDIT_VERIFY='1')
  measure([binary,'frozen',src,out/(case+'-'+mode),lib],out/(case+'-'+mode+'-process'))
 catalog=json.loads((out/(case+'-observe')/'cell-evolution.json').read_text())
 edges=collections.defaultdict(lambda:[0,0])
 for pat in catalog['patterns']:
  t=pat['transition'];edge=edges[(t['source_family'],t['target_family'])];edge[0]+=1;edge[1]+=pat['occurrences']
 lines=['digraph evolution {','rankdir=LR;','label="Observed anchored projections (not a proven state machine)";','node [shape=box];']
 for f in catalog['families']:
  label='T'+str(f['id'])+' / '+','.join(sorted({row[0] for row in f['rows']}))
  lines.append('t%d [label=%s];'%(f['id'],json.dumps(label)))
 for (srcid,dstid),(patterns,occurrences) in sorted(edges.items()):
  lines.append('t%d -> t%d [label="%d patterns / %d anchored observations"];'%(srcid,dstid,patterns,occurrences))
 lines.append('}');dot=out/(case+'-observe')/'transitions.dot';dot.write_text('\n'.join(lines)+'\n')
 if shutil.which('dot'):subprocess.run(['dot','-Tsvg',str(dot),'-o',str(dot.with_suffix('.svg'))],check=True)

 control=json.loads((out/(case+'-control')/'report.json').read_text())
 observed=json.loads((out/(case+'-observe')/'report.json').read_text())
 summary={k:v for k,v in catalog.items() if k not in ['patterns','families','rounds','rule_environment']}
 guards=collections.Counter(json.dumps({k:v for k,v in pat['transition'].items() if k in ['source_family','rule','source_support','before_aliases','read_guard','touched_slots','input_refinement']},sort_keys=True) for pat in catalog['patterns'])
 summary['read_shapes_with_multiple_observed_outcomes']=sum(n>1 for n in guards.values())
 summary['max_observed_outcomes_per_read_shape']=max(guards.values(),default=0)

 summary['family_dictionary']=catalog['families']
 summary['rule_environment']=catalog.get('rule_environment',[])
 summary.update(family_rule_edges=len({(p['transition']['source_family'],p['transition']['rule'],p['transition']['target_family']) for p in catalog['patterns']}),observed_read_shapes=len({json.dumps({k:v for k,v in p['transition'].items() if k in ['source_family','rule','source_support','before_aliases','read_guard','touched_slots','input_refinement']},sort_keys=True) for p in catalog['patterns']}),families=len(catalog['families']),source_sha256=hashlib.sha256(src.read_bytes()).hexdigest(),library_sha256=hashlib.sha256(lib.read_bytes()).hexdigest(),
  verification=verify(out/(case+'-control')/'verification-graph.json',out/(case+'-observe')/'verification-graph.json'),control_matches=control['matches'],observed_matches=observed['matches'],
  repeated_pattern_occurrences=sum(p['occurrences'] for p in catalog['patterns'] if p['distinct_source_cells']>1),anchored_occurrences=sum(p['occurrences'] for p in catalog['patterns']),
  input_refinement_patterns=sum(p['transition']['input_refinement'] for p in catalog['patterns']),examples=catalog['patterns'][:3],catalog=str(out/(case+'-observe')/'cell-evolution.json'))
 r['cases'][case]=summary;(out/'report.json').write_text(json.dumps(r,indent=2)+'\n')
 print(case,json.dumps({k:summary[k] for k in ['transition_patterns','patterns_repeated_across_instances','observed_instance_births','boundary_linked_births','verification']}),flush=True)

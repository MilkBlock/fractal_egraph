#!/usr/bin/env python3
"""Full keyed-read diagnostic vs untraced control, on a bounded math prefix."""
import argparse,csv,hashlib,json,os,subprocess
from pathlib import Path
from math_virtual import ROOT,measure
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',required=True);p.add_argument('--rounds',type=int,default=8);p.add_argument('--library',default='experiments/math_mini3/adaptive_runtime/bake_lib_round2.json');a=p.parse_args()
if a.rounds<1:p.error('rounds must be positive')
out=(ROOT/a.output).resolve();out.mkdir(parents=True,exist_ok=False)
source=(ROOT/'experiments/math_mini3/source.egg').read_text();assert source.count('(run 3)')==1
source=source.replace('(run 3)',f'(run {a.rounds})');src=out/'source.egg';src.write_text(source)
lib=(ROOT/a.library).resolve();binary=ROOT/'target/release/examples/adaptive_math';report={'rounds':a.rounds,'storage_threshold':2,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_sha256':hashlib.sha256(src.read_bytes()).hexdigest(),'library_sha256':hashlib.sha256(lib.read_bytes()).hexdigest(),'parent_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'working_tree_dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT))}
os.environ['EGG_LAYOUT_MIN_TEMPLATE_REUSE']='2'
os.environ['EGG_LAYOUT_AUDIT_VERIFY']='1'
for label,enabled in [('audit','1'),('control','0')]:
 os.environ['EGG_LAYOUT_AUDIT_ENCLOSURES']=enabled
 report[label]=measure([binary,'frozen',src,out/label,lib],out/(label+'-process'))
from math_graph_equal import verify
verified=verify(out/'audit/verification-graph.json',out/'control/verification-graph.json')
report['final_graph_exact_equal']=verified['exact_equal']
report['verified_enodes']=verified['enodes'];report['verified_eclasses']=verified['eclasses']
rows=[]
for s,t in zip(report['audit']['result']['samples'],report['control']['result']['samples']):
 assert (s['graph']['enodes'],s['graph']['eclasses'])==(t['graph']['enodes'],t['graph']['eclasses']), 'graph size mismatch'
 d=s['enclosure_audit'];m=d['matches'];e=d['committed_effectful_matches'];n=d['known_bodies_before_step']
 rows.append(dict(round=s['round'],prior_bodies=n,output_regions_changed=d['body_output_regions_changed'],lower_regions_changed=d['lower_input_regions_changed'],any_region_changed=d['any_region_changed'],changed_percent=100*d['any_region_changed']/n if n else None,productive_boundary_participants=d['productive_boundary_participating_bodies'],internal_matches=m.get('inside_one_prior_body',0),internal_effectful=e.get('inside_one_prior_body',0),boundary_matches=m.get('cross_prior_bodies',0)+m.get('boundary_with_external_facts',0),boundary_effectful=e.get('cross_prior_bodies',0)+e.get('boundary_with_external_facts',0),unknown=m.get('unknown_witness',0),match_delta_vs_control=s['matches']-t['matches']))
report['rows']=rows;report['graph_size_parity']=True;report['match_count_parity']=all(r['match_delta_vs_control']==0 for r in rows)
report['notes']=['Instances deduplicated by actual canonical fact sets, not abstract shape. Overlapping instances remain separate.', 'Region growth is a direct eclass-content change, not proof that every affected instance must be reripened. Lower-only classes are Math references without a result row in the body.', 'Committed effects mean Math inserts/updates or nonredundant unions, not logical matches. Notifications excluded.', 'Reads are compared with pre-step membership; newly created bodies cannot retroactively explain the current match.', 'Diagnostic tracing may retain extra logical matches; report differences and graph-size parity separately. Final constructor facts and eclass partition are checked exactly by ground constructor correspondence.', 'This is the current fixed 13-trigger library, not every possible partition or all saturated subgraphs.']
(out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
with (out/'rounds.csv').open('w') as f:w=csv.DictWriter(f,fieldnames=list(rows[0]),lineterminator="\n");w.writeheader();w.writerows(rows)
html='<!doctype html><meta charset="utf-8"><title>Enclosure boundary audit</title><style>body{font:16px system-ui;max-width:1150px;margin:32px auto;color:#203047}table{border-collapse:collapse;width:100%}td,th{padding:8px;text-align:right;border-bottom:1px solid #ddd}svg{width:100%;background:#f3f7fb}</style><h1>封装内部匹配与内容变化</h1><p>蓝：内容未变；橙：主体或下侧输入 e-class 有新增内容。实例可能重叠，变化不等于必须逐个重新 ripen。</p>'
html+='<svg viewBox="0 0 1000 270">'
for i,r in enumerate(rows):
 if not r['prior_bodies']:continue
 x=75+i*108;v=r['changed_percent'];h=v*1.8
 html+=f'<rect x="{x}" y="25" width="62" height="180" fill="#3074b5"/><rect x="{x}" y="{205-h}" width="62" height="{h}" fill="#d97920"/><text x="{x+31}" y="225" text-anchor="middle">轮 {r["round"]}</text><text x="{x+31}" y="247" text-anchor="middle">{v:.1f}%</text>'
html+='</svg><table><tr><th>轮次</th><th>既有实例</th><th>内容变化</th><th>内部匹配</th><th>内部实际提交</th><th>边界匹配</th><th>边界实际提交</th><th>trace/control 差</th></tr>'
for r in rows:html+='<tr>'+''.join(f'<td>{r[k]}</td>' for k in ['round','prior_bodies','any_region_changed','internal_matches','internal_effectful','boundary_matches','boundary_effectful','match_delta_vs_control'])+'</tr>'
html+='</table><h2>统计范围</h2>'+''.join('<p>'+n+'</p>' for n in report['notes'])
(out/'index.html').write_text(html)
print(json.dumps(rows,indent=2))

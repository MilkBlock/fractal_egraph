"""Summarize one interactive diagnostic; no averaging, trace or coverage run."""
import argparse
import csv
import datetime
import hashlib
import json
from pathlib import Path
import re
import subprocess
from zoneinfo import ZoneInfo

p=argparse.ArgumentParser()
p.add_argument('live',type=Path)
p.add_argument('control',type=Path)
p.add_argument('--output',type=Path,required=True)
a=p.parse_args()
root=Path(__file__).resolve().parents[2]
live=json.loads(a.live.read_text());control=json.loads(a.control.read_text())
matches=lambda report:{r['rule']:r['matches'] for r in report['rules']}
assert matches(live)==matches(control),'interactive schedule changed rule match counts'
assert live['tables']==control['tables'] and live['table_rows']==control['table_rows']
def ruleset(rule):
    found=re.search(r':ruleset\s+([^\s)]+)',rule)
    return found.group(1) if found else 'default'
total=sum(r['search_and_apply_seconds'] for r in live['rules'])
families={
 'candidate_construction_and_selection':lambda r:ruleset(r['rule']) in {'terms','terms-helpers','terms-helpers-helpers'},
 'expr_size':lambda r:'(set (Expr-size' in r['rule'],
 'subtuple_affine_unfold':lambda r:r['rule'].startswith('(rewrite (SubTuple expr a b)'),
 'subtuple_adjacent_merge':lambda r:r['rule'].startswith('(rewrite (Concat (SubTuple expr a b)'),
}
groups={}
for name,predicate in families.items():
    rows=[r for r in live['rules'] if predicate(r)]
    seconds=sum(r['search_and_apply_seconds'] for r in rows)
    groups[name]=dict(rules=len(rows),matches=sum(r['matches'] for r in rows),search_and_apply_seconds=seconds,share_of_per_rule_time=seconds/total)
hot=max(live['packets'],key=lambda row:row['rule_search_and_apply_seconds'])
summary=dict(date=str(datetime.datetime.now(ZoneInfo('Asia/Shanghai')).date()),scope='Fresh interactive replay of saved production raytrace Pass3, not a new full three-pass timing or a certified affine Cell discovery.',
             logical_matches=live['logical_matches'],per_rule_search_and_apply_seconds=total,
             observed_rules=len(live['rules']),executed_packets=len(live['packets']),planned_packets=live['packet_count'],
             table_rows=live['table_rows'],tables=live['tables'],families=groups,
             top_rules_by_time=live['rules'][:12],top_rules_by_matches=sorted(live['rules'],key=lambda r:-r['matches'])[:8],
             hottest_packet=hot,control=dict(all_per_rule_matches_equal=True,total_table_rows_equal=True,selected_table_sizes_equal=True,
             full_graph_equality_checked=False,extraction_checked=False),
             limitations=['Search+apply contains RHS and matching management; no per-rule pure-Join timing.',
                          'Existing global atomic probe counters add overhead; timings diagnose this local instrumented replay, not pristine production speed.',
                          'Match counts are complete LHS bindings reported by the runtime, not distinct repeated-binding counts or committed mutations.',
                          'The terms family is piecewise affine cost propagation plus argmin and structural Term payload; affine closure has not been proved.'])
paths={
    'raytrace_source':root.parent/'check_statewalk/benchmarks/passing/raytrace/raytrace.rs',
    'production_pass3_source':root/'out/eggcc-raytrace-pass-audit/raytrace/pass-3.egg',
    'flattened_replay_source':a.live.resolve().parent.parent/'pass-3-flat.egg',
    'runtime':root/'target/release/examples/rule_repl',
}
summary['provenance']={name:dict(path=str(path),sha256=hashlib.sha256(path.read_bytes()).hexdigest()) for name,path in paths.items()}
summary['provenance']['kernel_diff_sha256']=hashlib.sha256(subprocess.check_output(['git','diff','HEAD','--','egglog'],cwd=root)).hexdigest()
summary['provenance']['base_commit']=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
capture=json.loads((root/'out/eggcc-raytrace-pass-audit/measurement.json').read_text())
assert summary['provenance']['raytrace_source']['sha256']==capture['source_sha256'],'saved pass input belongs to a different source version'
summary['provenance']['saved_production_capture']=capture
a.output.mkdir(parents=True,exist_ok=True)
(a.output/'results.json').write_text(json.dumps(summary,indent=2)+'\n')
with (a.output/'rules.csv').open('w',newline='') as f:
    writer=csv.writer(f,lineterminator='\n');writer.writerow(['time_rank','ruleset','logical_matches','search_and_apply_seconds','rule_sha256','rule'])
    for i,row in enumerate(live['rules'],1):
        text=row['rule'];rs=ruleset(text)
        digest=hashlib.sha256(text.encode()).hexdigest()
        if rs=='initialization':text='Initialization source facts omitted; full rule is in the captured pass and live report.'
        writer.writerow([i,rs,row['matches'],row['search_and_apply_seconds'],digest,text])
print(json.dumps({'families':groups,'hottest_packet':hot['packet'],'matches':live['logical_matches']},indent=2))

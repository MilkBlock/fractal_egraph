"""Compare the saved per-rule reports of the three actual production pass inputs."""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re

p=argparse.ArgumentParser()
p.add_argument('--input',type=Path,default=Path('out/raytrace-interactive-20261010'))
p.add_argument('--output',type=Path,default=Path('experiments/raytrace_interactive'))
a=p.parse_args()
root=Path(__file__).resolve().parents[2]
previous=json.loads((root/'experiments/raytrace_interactive/results.json').read_text())
runtime=root/'target/release/examples/rule_repl'
assert hashlib.sha256(runtime.read_bytes()).hexdigest()==previous['provenance']['runtime']['sha256'],'runtime changed since Pass3 diagnostic'
def ruleset(text):
    m=re.search(r':ruleset\s+([^\s)]+)',text)
    return m.group(1) if m else 'default'
def family(text):
    rs=ruleset(text)
    if rs in {'terms','terms-helpers','terms-helpers-helpers'}:return 'candidate_construction_and_selection'
    if '(set (Expr-size' in text:return 'expr_size'
    return 'always_run_other' if rs=='always-run' else rs
reports=[];csv_rows=[]
for i,path in [(1,a.input/'pass1/final.json'),(2,a.input/'pass2/final.json'),(3,a.input/'live/complete.json')]:
    report=json.loads(path.read_text());total=sum(r['search_and_apply_seconds'] for r in report['rules'])
    assert sum(r['matches'] for r in report['rules'])==report['logical_matches']
    groups={}
    for rank,r in enumerate(report['rules'],1):
        g=groups.setdefault(family(r['rule']),dict(matches=0,seconds=0.0,rule_count=0))
        g['matches']+=r['matches'];g['seconds']+=r['search_and_apply_seconds'];g['rule_count']+=1
        source=r['rule'];digest=hashlib.sha256(source.encode()).hexdigest();rs=ruleset(source)
        if rs=='initialization':source='Initialization facts omitted; see captured pass source and SHA-256.'
        csv_rows.append([i,rank,rs,r['matches'],r['search_and_apply_seconds'],digest,source])
    for group in groups.values():group['percent']=100*group['seconds']/total
    source=root/f'out/eggcc-raytrace-pass-audit/raytrace/pass-{i}.egg'
    flat=a.input/(f'pass{i}/source.egg' if i<3 else 'pass-3-flat.egg')
    reports.append(dict(pass_number=i,logical_matches=report['logical_matches'],observed_rules=len(report['rules']),
                        per_rule_search_and_apply_seconds=total,table_rows=report['table_rows'],tables=report['tables'],
                        groups=groups,top_rules=report['rules'][:8],raw_report=str(path),
                        production_pass_source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                        replay_source_sha256=hashlib.sha256(flat.read_bytes()).hexdigest()))
combined=dict(scope='One replay per saved production pass input on the same local instrumented native runtime. Pass3 reuses the preceding diagnostic; Pass1 and Pass2 are newly replayed. Not a fresh end-to-end eggcc run.',
              runtime_sha256=previous['provenance']['runtime']['sha256'],passes=reports,
              lhs_table_atoms=dict(Smaller=3,better_candidate=4,Bop_candidate=5,Concat_candidate=5,Top_candidate=7),
              limitations=['Table-atom counts describe logical LHS decomposition, not physical stages or visited row counts.',
                           'Times include RHS and match management plus existing counter overhead; no pure per-rule Join timing.',
                           'Piecewise affine describes only fixed-candidate scalar cost updates; no affine closed Cell has been certified.',
                           'No fresh extraction or output quality comparison; each saved pass already incorporates preceding production extraction.'])
a.output.mkdir(parents=True,exist_ok=True)
(a.output/'passes.json').write_text(json.dumps(combined,indent=2)+'\n')
with (a.output/'all-pass-rules.csv').open('w',newline='') as f:
    w=csv.writer(f,lineterminator='\n');w.writerow(['pass','time_rank','ruleset','logical_matches','search_and_apply_seconds','rule_sha256','rule']);w.writerows(csv_rows)
for r in reports:
    g=r['groups']['candidate_construction_and_selection']
    print(r['pass_number'],round(r['per_rule_search_and_apply_seconds'],3),g['matches'],round(g['seconds'],3),round(g['percent'],2))

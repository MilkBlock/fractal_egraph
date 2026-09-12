"""Independent finite expansion check against the original tier-1 edge inventory."""
import json
from collections import defaultdict
from pathlib import Path
OUT=Path(__file__).parent
steps=json.loads((OUT/'higher_steps.json').read_text());higher=json.loads((OUT/'higher_native.json').read_text())['higher_rules']
edges=defaultdict(set)
for s in steps:edges[s['parent'],s['extension']].add((s['out'],s['binding']))
checks=[]
for h in higher:
    reachable={out for out,b in edges[h['ctx'],h['operator']] if b==h['binding']}
    for _ in range(h['count']-1):reachable={out for mid in reachable for out,b in edges[mid,h['operator']]}
    assert set(h['represents'])<=reachable,(h,reachable)
    checks.append({'count':h['count'],'ctx':h['ctx'],'operator':h['operator'],'exact_original_targets':h['represents']})
(OUT/'higher_validation.json').write_text(json.dumps({'passed':len(checks),'checks':checks,'scope':'Every native HigherRule expands through existing unary tier-1 edges to its exact original target. Finite representation validation, not arbitrary-k closure proof.'},indent=2)+'\n')
print('HigherRule finite expansion checks:',len(checks))

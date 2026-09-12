"""Build finite symbolic boundary-seed tests; native egglog parses all rules."""
import json,re
from pathlib import Path
OUT=Path(__file__).parent
r=json.loads((OUT/'combined.json').read_text());cases=[]
for c in r['combinations']:
 for k,v in enumerate(c['variants']):
    variables=sorted(set(re.findall(r'\bv\d+\b',' '.join(v['body']))),key=lambda x:int(x[1:]))
    setup=[f'(let {x} (Var "boundary-{x}"))' for x in variables]
    setup += ['(union '+x[3:] for x in v['body']]
    env={x:x for x in variables};checks=[]
    def expand(s):return re.sub(r'\bv\d+\b',lambda m:env[m[0]],s)
    for h in v['head']:
        if h.startswith('(let '):
            _,var,expr=h.split(' ',2);expr=expand(expr[:-1]);env[var]=expr
            checks.append('(check (= __present '+expr+'))')
        else:checks.append('(check (= '+expand(h[len('(union '):])+')')
    cases.append({'name':c['name']+'_v'+str(k),'rule':v['egg'],'setup':'\n'.join(setup),
        'schedule':'\n'.join('(run source-'+s['rule']+' 1)' for s in v['steps']),'checks':'\n'.join(checks)})
(OUT/'validation_cases.json').write_text(json.dumps(cases)+'\n')

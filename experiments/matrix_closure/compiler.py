"""Compile a fixed typed structural carrier to recurrent Boolean contexts.

Accepted rules have one immutable constructor anchor, unary existence premises,
and only unary existence outputs. Grounding scans anchor rows, never searches
the changing predicates. Learned conjunction separators are certified before
their scores can authorize any effect.
"""
import hashlib
import json
from collections import defaultdict
import numpy as np

def identity(v):
    return (v['sort'], 'eq', v['eq']) if 'eq' in v else (v['sort'], 'literal', json.dumps(v['value'],sort_keys=True))

def separator(k, regularization=1.0):
    # Ordinary ridge, trained on the rule's Boolean condition domain, not on
    # expected answers from the graph to be tested.
    # Exact sufficient statistics of the full Boolean domain. No 2**k examples
    # are enumerated. Only the all-one corner has label one.
    inverse_n=float(np.exp2(-float(k)))
    if inverse_n==0:raise ValueError('Boolean-domain statistics exceed numeric range')
    gram=np.full((k+1,k+1),.25);gram[0,:]=.5;gram[:,0]=.5
    np.fill_diagonal(gram,.5);gram[0,0]=1.
    raw=np.linalg.solve(gram+regularization*inverse_n*np.eye(k+1),np.full(k+1,inverse_n))
    scale=1./float(np.max(np.abs(raw)))
    theta=(raw*scale).astype(np.float16)
    if not np.all(np.isfinite(theta)) or (k and not np.all(theta[1:]>0)):
        raise ValueError('ridge model has no conjunction certificate')
    # With positive coefficients, the largest negative Boolean corner omits
    # precisely the smallest coefficient. This certificate covers all corners.
    positive=float(theta.astype(np.float64).sum())
    negative=positive-float(theta[1:].min()) if k else positive-1
    threshold=(positive+negative)/2
    error=float(8*np.finfo(np.float32).eps*float(np.abs(theta.astype(np.float64)).sum()))
    margin=min(positive-threshold,threshold-negative)
    if margin<=error:raise ValueError('insufficient FP32 separation margin')
    return {'arity':k,'coefficients':theta.view(np.uint16).tolist(),'threshold':threshold,
            'positive_min':positive,'negative_max':negative,'roundoff_bound':error,
            'margin':margin,'truth_assignments':1<<k,'regularization':regularization,
            'positive_score_rescaling':scale}

def compile_context(core,ruleset):
    selected=next(s for s in core['rulesets'] if s['name']==ruleset)['rules']
    tables={t['name']:t for t in core['tables']}
    outputs=set()
    for r in selected:
        for a in r['head']:
            if a['op']=='copy':continue
            if a['op']!='let' or 'table' not in a.get('call',{}):
                raise ValueError('closure output must be a unary constructor fact')
            outputs.add(a['call']['table'])
    for name in outputs:
        t=tables[name]
        if t['kind']!='constructor' or len(t['schema'])!=2 or any(s['kind']!='eq' for s in t['schema']):
            raise ValueError('requires unary equality-typed existence predicates')
    if any(r['subsumed'] and r['table'] in outputs for r in core['initial']):
        raise ValueError('hidden output predicates require explicit visibility semantics')
    grouped=defaultdict(list)
    for index,row in enumerate(core['initial']):
        if not row['subsumed']:grouped[row['table']].append((index,row))
    facts={};clauses={};static_rows=0;rules=[]
    def coordinate(table,value):
        key=(table,identity(value))
        if key not in facts:facts[key]=(len(facts),value)
        return facts[key][0]
    for rid,r in enumerate(selected):
        if any('table' not in a['call'] for a in r['body']):
            raise ValueError('primitive conditions require an additional exact operator')
        anchors=[a for a in r['body'] if a['call']['table'] not in outputs]
        if len(anchors)!=1:raise ValueError('requires one immutable structural anchor')
        anchor=anchors[0];reads=[a for a in r['body'] if a is not anchor]
        rules.append({'rule':rid,'name':r['name'],'anchor':anchor['call']['table'],'premises':len(reads)})
        for row_index,row in grouped[anchor['call']['table']]:
            static_rows+=1;binding={};ok=True
            for term,value in zip(anchor['args'],row['values']):
                if term['sort']['name']!=value['sort']:raise ValueError('typed anchor mismatch')
                if 'literal' in term:
                    ok &= 'value' in value and term['literal']==value['value']
                elif term['var'] in binding:
                    ok &= identity(binding[term['var']])==identity(value)
                else:binding[term['var']]=value
            if not ok:continue
            requirements=[];private_returns=set()
            for a in reads:
                key,result=a['args'];name=a['call']['table']
                if 'var' not in key or key['var'] not in binding:
                    raise ValueError('existence key is not grounded by the structural anchor')
                if 'var' not in result or result['var'] in binding or result['var'] in private_returns:
                    raise ValueError('predicate result has an observable constraint')
                private_returns.add(result['var']);requirements.append(coordinate(name,binding[key['var']]))
            targets=[]
            for a in r['head']:
                if a['op']=='copy':
                    if a['arg'].get('var') not in binding:raise ValueError('effect depends on predicate identity')
                    binding[a['dst']]=binding[a['arg']['var']]
                else:
                    if len(a['args'])!=1 or a['args'][0].get('var') not in binding:
                        raise ValueError('output is not grounded by the anchor')
                    targets.append(coordinate(a['call']['table'],binding[a['args'][0]['var']]))
                    # A fresh predicate identity must not feed another action.
            requirements=tuple(sorted(set(requirements)))
            for target in targets:
                clauses.setdefault((requirements,target),[]).append({'rule':rid,'row':row_index})
    initial=[0]*len(facts)
    for name in sorted(outputs):
        for _,row in grouped[name]:
            slot=coordinate(name,row['values'][0])
            if slot>=len(initial):initial.extend([0]*(slot+1-len(initial)))
            initial[slot]=1
    groups=[]
    for k in sorted({len(p) for p,_ in clauses}):
        items=[(p,t,w) for (p,t),w in clauses.items() if len(p)==k]
        groups.append({'arity':k,'separator':separator(k),
                       'premises':[list(p) for p,_,_ in items],
                       'targets':[t for _,t,_ in items],
                       'witnesses':[w for _,_,w in items]})
    coordinates=[None]*len(facts)
    for (table,_),(slot,value) in facts.items():coordinates[slot]={'table':table,'input':value}
    return {'schema':'matrix-closure/v1','ruleset':ruleset,'rules':rules,
            'coordinates':coordinates,'initial':initial,'groups':groups,
            'anchor_rows_read':static_rows,'clauses':len(clauses),
            'expanded_ground_witnesses':sum(map(len,clauses.values())),
            'rule_sha256':hashlib.sha256(json.dumps(selected,sort_keys=True).encode()).hexdigest(),
            'scope':'fixed structural carrier; monotone unary predicates; no new structural nodes or union'}

def reference(program):
    state=program['initial'][:];history=[]
    while True:
        after=state[:]
        for group in program['groups']:
            for deps,target in zip(group['premises'],group['targets']):
                if all(state[d] for d in deps):after[target]=1
        history.append(after)
        if after==state:return history
        state=after

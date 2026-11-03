"""Rule-derived residual incidence matrices on PRE-stage facts.

The compiler emits column selectors once per rule. It does not match individual
bindings. A device row program gathers unary premises from native table rows.
Only re-emission of the same structural anchor and unary predicate outputs is
admitted; unsupported rules are listed, never silently made unconditional.
"""
from collections import defaultdict, deque
import json
import numpy as np
from features import identity


def compile_recipes(core, target='ExprIsPure', ruleset='always-run'):
    tables={t['name']:t for t in core['tables']}
    rules=next(s['rules'] for s in core['rulesets'] if s['name']==ruleset)
    family={target}
    def unary(name):
        t=tables[name]
        return len(t['schema'])==2 and t['schema'][0]['kind']=='eq' and t['schema'][1]['name'].startswith('___')
    # Include mutually dependent unary existence predicates, without name-based
    # purity theory or learning a transition from post-stage facts.
    while True:
        previous=family.copy()
        for r in rules:
            if any(a.get('call',{}).get('table') in family for a in r['head']):
                family.update(a['call']['table'] for a in r['body']
                              if 'table' in a['call'] and unary(a['call']['table']))
        if previous==family:break
    recipes=[];rejected=[];selected=[]
    for rid,r in enumerate(rules):
        if not any(a.get('call',{}).get('table') in family for a in r['head']):continue
        selected.append(r)
        try:
            anchors=[a for a in r['body'] if a['call'].get('table') not in family]
            if len(anchors)!=1 or 'table' not in anchors[0]['call']:
                raise ValueError('multiple structural atoms or primitive guard')
            anchor=anchors[0];name=anchor['call']['table']
            if tables[name]['kind']!='constructor':raise ValueError('mutable structural function anchor')
            if any('literal' in a for a in anchor['args']):raise ValueError('literal anchor constraint')
            columns={};aliases=[]
            for i,a in enumerate(anchor['args']):
                if a['var'] in columns:aliases.append((columns[a['var']],i))
                else:columns[a['var']]=i
            deps=[];private=set()
            for a in r['body']:
                if a is anchor:continue
                key,returned=a['args']
                if key.get('var') not in columns or returned.get('var') in columns or returned.get('var') in private:
                    raise ValueError('predicate requires additional binding or constrained result')
                private.add(returned['var']);deps.append((a['call']['table'],columns[key['var']]))
            aliases_of=dict(columns);outputs=[]
            for a in r['head']:
                if a['op']=='copy':aliases_of[a['dst']]=aliases_of[a['arg']['var']]
                elif a['op']=='let' and a['call'].get('table')==name:
                    args=[aliases_of[x['var']] for x in a['args']]
                    if args != [columns[x['var']] for x in anchor['args'][:-1]]:
                        raise ValueError('creates a different structural anchor')
                    aliases_of[a['dst']]=len(anchor['args'])-1
                elif a['op']=='let' and a['call'].get('table') in family:
                    outputs.append((a['call']['table'],aliases_of[a['args'][0]['var']]))
                else:raise ValueError('non-unary or structural mutation effect')
            for output in outputs:
                recipes.append({'source_rule':rid,'name':r['name'],'anchor':name,'width':len(anchor['args']),
                                'aliases':aliases,'deps':deps,'output':output})
        except (ValueError,KeyError) as error:
            rejected.append({'source_rule':rid,'name':r['name'],'reason':str(error)})
    return {'family':sorted(family),'recipes':recipes,'rejected':rejected,'selected_rules':len(selected),
            'target':target,'ruleset':ruleset}


def pack_context(core, snapshot, keys, program):
    """Typed coordinate assignment and raw row copying; no row-to-rule Join."""
    family=program['family'];pid={p:i for i,p in enumerate(family)}
    needed={p['anchor'] for p in program['recipes']}|set(family)
    rows=[r for r in snapshot['rows'] if not r['subsumed'] and r['table'] in needed]
    # Slot 0 is unused. Cross-sort values never share a slot accidentally.
    values={}
    def slot(v):
        k=identity(v)
        if k not in values:values[k]=len(values)+1
        return values[k]
    raw=defaultdict(list)
    for row in rows:raw[row['table']].append([slot(v) for v in row['values']])
    for key in keys:
        if key not in values:values[key]=len(values)+1
    n=len(values)+1;seed=np.zeros(n*len(family),np.float32)
    for p in family:
        for r in raw[p]:seed[pid[p]*n+r[0]]=1
    descriptors=[];matrix=[];tasks=0;bytecode=[];used_tables={}
    for recipe in program['recipes']:
        name=recipe['anchor']
        if name not in used_tables:
            used_tables[name]=len(matrix)
            for row in raw[name]:matrix.extend(row)
        start=len(bytecode)
        for a,b in recipe['aliases']:bytecode.extend([a,b])
        for p,col in recipe['deps']:bytecode.extend([pid[p],col])
        count=len(raw[name]);tasks+=count
        outp,outcol=recipe['output']
        descriptors.append([tasks-count,tasks,used_tables[name],recipe['width'],start,
                            len(recipe['aliases']),len(recipe['deps']),pid[outp],outcol])
    queries=np.array([pid[program['target']]*n+values[k] for k in keys],np.uint32)
    return {'state_width':n,'predicates':len(family),'seed':seed,
            'rows':np.array(matrix,np.uint32),'code':np.array(bytecode,np.uint32),
            'programs':np.array(descriptors,np.uint32).reshape(-1,9),'queries':queries,
            'raw_anchor_rows':sum(len(raw[t]) for t in used_tables),'tasks':tasks,
            'coordinate_keys':list(values),'program':program}


def incidence(context, unique=True):
    """Independent CPU diagnostic; not used to supply GPU's rule bindings."""
    clauses=[]
    for d in context['programs']:
        start,end,base,width,pc,na,nd,pred,col=map(int,d)
        for at in range(end-start):
            row=context['rows'][base+at*width:base+(at+1)*width];code=context['code']
            if any(row[code[pc+2*j]]!=row[code[pc+2*j+1]] for j in range(na)):continue
            occurrences=[int(code[pc+2*na+2*j])*context['state_width']+
                         int(row[code[pc+2*na+2*j+1]]) for j in range(nd)]
            deps=tuple(sorted(set(occurrences))) if unique else tuple(occurrences)
            clauses.append((pred*context['state_width']+int(row[col]),deps))
    return clauses


def project(context, rounds=8):
    """CPU oracle for exact fixed-depth nonlinear projection, not native truth."""
    clauses=incidence(context);state=context['seed'].copy();columns=[state.copy()]
    for _ in range(rounds):
        nxt=state.copy()
        for target,deps in clauses:
            if all(state[d]>=1 for d in deps):nxt[target]=1
        state=nxt;columns.append(state.copy())
    return np.stack(columns,axis=1)[context['queries']]


def frozen_closure(context):
    """Work-list diagnostic bound of the admitted PRE-stage support graph."""
    clauses=incidence(context);subs=defaultdict(list);remaining=[];active=context['seed'].astype(bool).copy()
    for i,(target,deps) in enumerate(clauses):
        remaining.append(sum(not active[d] for d in deps))
        for d in deps:subs[d].append(i)
    queue=deque(t for i,(t,_) in enumerate(clauses) if remaining[i]==0 and not active[t])
    while queue:
        target=queue.popleft()
        if active[target]:continue
        active[target]=True
        for i in subs[target]:
            remaining[i]-=1
            if remaining[i]==0:queue.append(clauses[i][0])
    return active[context['queries']],clauses

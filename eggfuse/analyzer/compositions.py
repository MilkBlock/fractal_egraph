"""Observed, checked multi-rule programs; never an inferred saturation endpoint.

Selection is from recorded rule handoffs. Proof is from the complete typed rules:
every consumer atom is either an immutable producer fact or an exact keyed read.
The source schedule still decides when each step may execute.
"""
from collections import Counter, defaultdict
from .multistage import sources, extend, symbol
from eggfuse.ir.recipe import digest

SCHEMA = 'eggfuse-compositions/v1'


def build(recipe, marker_names=(), evidence=None):
    evidence = evidence or {'recipe_sha256':recipe['sha256'],'pairs':[]}
    if evidence['recipe_sha256'] != recipe['sha256']:
        raise ValueError('composition observations belong to a different typed recipe')
    rules=recipe['rules']; tables={t['name']:t for t in recipe['tables']}
    markers=set(marker_names)
    removed={e.get('call',{}).get('table') for r in rules for e in r['effects'] if e['op'] in ('delete','subsume')}
    def immutable(t):
        return t not in removed and (tables[t]['kind']=='constructor' and tables[t].get('merge_expr') is None
                                    or t in markers and tables[t].get('merge_expr')=={'var':'old'})
    def original(i): return not rules[i]['metadata']['source_set'].startswith('cell-')
    pairs=sorted({tuple(p) for p in evidence['pairs']})
    if any(len(p)!=2 or any(not isinstance(i,int) or i<0 or i>=len(rules) for i in p) for p in pairs):
        raise ValueError('invalid observed composition pair')
    edges=[]; rejected=Counter()
    for producer,consumer in pairs:
        if not original(producer) or not original(consumer) or producer==consumer:
            rejected['auxiliary_or_same_rule']+=1;continue
        src=rules[producer]; dst=rules[consumer]
        # Copy aliases and completed producer unions are equations on source slots.
        copies={e['dst']:e['arg'] for e in src['effects'] if e['op']=='copy'}
        def copy(a):
            while 'slot' in a and a['slot'] in copies:a=copies[a['slot']]
            return a
        parents={}; terms={}
        def root(a):
            a=copy(a);k=symbol(a);terms[k]=a
            while k in parents:k=parents[k]
            return k
        for e in src['effects']:
            if e['op']=='union':
                a,b=map(root,e['args'])
                if a!=b:parents[max(a,b)]=min(a,b)
        norm=lambda a:terms[root(a)]
        local=[{**s,'args':[norm(a) for a in s['args']]} for s in sources(src,tables) if immutable(s['table'])]
        anchors=[s for s in local if s['kind']=='effect']
        accepted=0
        for head in anchors:
            for anchor,a in enumerate(dst['query']):
                mapping=extend(a,head,{})
                if mapping is None:continue
                known={v['slot'] for v in a['args'] if 'slot' in v}
                remaining=set(range(len(dst['query'])))-{anchor}; instructions=[]
                while remaining:
                    ready=[i for i in sorted(remaining) if all('slot' not in v or v['slot'] in known for v in dst['query'][i]['args'][:-1])]
                    if not ready:break
                    i=ready[0];atom=dst['query'][i];provided=None
                    for s in local:
                        if all('slot' not in v or v['slot'] in mapping for v in atom['args'][:-1]):
                            new=extend(atom,s,mapping)
                            if new is not None:provided=(s,new);break
                    if provided:
                        s,mapping=provided
                        instructions.append(dict(atom=i,kind='known',source_kind=s['kind'],source_index=s['index']))
                    else:instructions.append(dict(atom=i,kind='query'))
                    known.update(v['slot'] for v in atom['args'] if 'slot' in v);remaining.remove(i)
                if remaining:
                    rejected['unbound_inverse_or_disconnected']+=1;continue
                # Reuse only already committed immutable constructions. Never use
                # a future consumer union to rewrite an earlier RHS lookup.
                reuse=[]; effect_mapping=dict(mapping)
                for op,e in enumerate(dst['effects']):
                    if e['op']=='copy':
                        v=e['arg']
                        if 'slot' not in v or v['slot'] in effect_mapping:effect_mapping[e['dst']]=effect_mapping.get(v.get('slot'),v)
                    elif e['op']=='let' and tables.get(e['call'].get('table'),{}).get('kind')=='constructor':
                        if not all('slot' not in v or v['slot'] in effect_mapping for v in e['args']):continue
                        key=[effect_mapping.get(v.get('slot'),v) for v in e['args']]
                        for s in local:
                            if s['table']==e['call']['table'] and s['args'][:-1]==key:
                                reuse.append(dict(op=op,source_kind=s['kind'],source_index=s['index']))
                                effect_mapping[e['dst']]=s['args'][-1];break
                edges.append(dict(producer=producer,effect=head['index'],consumer=consumer,anchor=anchor,
                                  instructions=instructions,reuse=reuse))
                accepted+=1
        if not accepted:rejected['no_complete_anchor_program']+=1
    edges.sort(key=lambda e:(e['producer'],e['effect'],e['consumer'],e['anchor']))
    # Shared finite graph, not every path through the graph. A cursor records
    # actual completed steps; cycles do not imply an already saturated Cell.
    adjacency=defaultdict(set)
    for e in edges:adjacency[e['producer']].add(e['consumer']);adjacency[e['consumer']].add(e['producer'])
    programs=[];rule_program={}
    for start in sorted(adjacency):
        if start in rule_program:continue
        component=set();todo=[start]
        while todo:
            x=todo.pop()
            if x in component:continue
            component.add(x);todo.extend(adjacency[x]-component)
        pid=len(programs)
        for x in component:rule_program[x]=pid
        entries=[dict(rule=i,atom=j,marker=a['call']['table'],arguments=a['args']) for i in sorted(component)
                 for j,a in enumerate(rules[i]['query']) if a['call'].get('table') in markers]
        def equalities(rule):
            result=[]
            for j,e in enumerate(rule['effects']):
                if e['op']=='union':result.append(dict(op=j,kind='union',args=e['args'],activation='after-source-Run-repair'))
                elif e['op']=='set' and tables[e['call']['table']]['kind']=='constructor':
                    result.append(dict(op=j,kind='constructor-result',table=e['call']['table'],args=e['args'],result=e['value'],activation='after-source-Run-repair'))
            return result
        steps=[dict(rule=i,effects=list(range(len(rules[i]['effects']))),
                    guards=[j for j,a in enumerate(rules[i]['query']) if 'primitive' in a['call']],
                    equalities=equalities(rules[i]))
               for i in sorted(component)]
        programs.append(dict(id=pid,steps=steps,marker_entries=entries))
    for e in edges:e['program']=rule_program[e['producer']]
    result=dict(schema=SCHEMA,recipe_sha256=recipe['sha256'],control=recipe['control'],marker_tables=sorted(markers),
                evidence=evidence,programs=programs,edges=edges,rejected=dict(rejected),
                contract=dict(activation='original Run; full correlated anchor binding',
                              completion='certified rule steps, not a saturation certificate',
                              effects='complete original RHS in original order',
                              late_input='original delta pivots remain enabled',
                              unknown='ordinary matching; no omitted binding'))
    result['sha256']=digest(result)
    return result


def validate(plan,recipe,marker_names=()):
    if plan!=build(recipe,marker_names,plan['evidence']):raise ValueError('multi-rule recipe proof differs')
    return plan

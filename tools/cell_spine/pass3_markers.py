"""Analyze-witnessed shared constructor views in a mixed, scheduled .egg.

Unlike compact Cell execution, this adapter keeps all original facts/effects.
Exact view completion before each original Run covers initial and union-created
bindings. Source publishers only add positive tuples; they never replace that
completeness obligation. Mutable/subsumable input tables are not view sources.
"""
import argparse
from collections import defaultdict
import itertools
import json
from pathlib import Path

from . import compiler, solver, takeover as t
from .terms import text,walk,actions
from .lhs_coverage import Facts
from .representation import Congruence

PREFIX='CellSpineView'
REPAIR='cell-spine-complete'
SHADOW='cell-spine-original'


def vars_in(e):return {x['name'] for x in walk(e) if x['kind']=='var'}


def render_rule(r,body,head,ruleset=None):
    action_text=[f'(panic {json.dumps(a["message"])})' if a['kind']=='panic' else actions([a])[0] for a in head]
    result=f'(rule ({" ".join(t.fact_text(f) for f in body)}) ({" ".join(action_text)})'
    name=r['ruleset'] if ruleset is None else ruleset
    if name:result+=f' :ruleset {name}'
    return result+f' :name {json.dumps(r["name"])}'+(' :naive' if r.get('naive') else '')+')'


def flatten(rule,sig,globals_=frozenset()):
    used={v['name'] for f in rule['body'] for e in (f['args'] if f['kind']=='eq' else [f['expr']]) for v in walk(e) if v['kind']=='var'}
    if '_' in used:return None
    atoms=[];types={};memo={}
    def visit(e):
        if e['kind']!='call':return e
        e=dict(e,args=[visit(a) for a in e['args']])
        s=sig.get(e['head'])
        if not s or s['kind']!='constructor':return e
        if any(a['kind']=='call' for a in e['args']):return e
        key=text(e)
        if key in memo:return memo[key]
        name=f'cell_spine_v{len(used)}'
        while name in used:name+='x'
        used.add(name);v=t.var(name);memo[key]=v
        for a,sort in zip(e['args']+[v],s['input']+[s['output']]):
            if a['kind']=='var':
                if a['name'] in types and types[a['name']]!=sort:raise ValueError('inconsistent constructor slot types')
                types[a['name']]=sort
        atoms.append(dict(kind='eq',args=[v,e]));return v
    rest=[]
    for f in rule['body']:
        rest.append(dict(f,args=[visit(e) for e in f['args']]) if f['kind']=='eq' else dict(f,expr=visit(f['expr'])))
    # Use native variable-equality semantics. Do not infer injectivity.
    aliases=t.variable_aliases(rest);groups=defaultdict(set)
    for f in rest:
        if f['kind']=='eq' and all(e['kind']=='var' for e in f['args']):
            for e in f['args']:groups[t.replace(e,aliases)['name']].add(e['name'])
    subst={}
    for root,names in groups.items():
        fixed=sorted(names & globals_);representative=fixed[0] if fixed else root
        # Globals denote values, not bindable pattern variables. In particular
        # two different globals keep their equality guard rather than renaming.
        subst.update({name:t.var(representative) for name in names-globals_ if name!=representative})
    atoms=[t.remap_fact(f,subst) for f in atoms]
    rest=[t.remap_fact(f,subst) for f in rest]
    typed={}
    for name,sort in types.items():
        v=t.replace(t.var(name),subst)
        if v['kind']=='var':typed[v['name']]=sort
    rest=[f for f in rest if not (f['kind']=='eq' and f['args'][0]==f['args'][1]) and
          not (f['kind']=='fact' and f['expr']['kind']=='var')]
    return dict(rule,body=atoms+rest,atom_count=len(atoms),types=typed,
                head=[a if a['kind']=='panic' else t.remap_action(a,subst) for a in rule['head']])


def canonical(pair,types):
    choices=[]
    for facts in (pair,list(reversed(pair))):
        available=set.union(*(vars_in(e) for e in facts[0]['args']))
        # Match the existing fully-keyed Cell projection contract. The next
        # constructor is a functional lookup, never a new reverse-fiber join.
        if not set.union(set(),*(vars_in(e) for e in facts[1]['args'][1]['args']))<=available:continue
        binding={};args=[];sorts=[]
        for f in facts:
            for e in f['args']:
                for v in walk(e):
                    if v['kind']=='var' and v['name'] not in binding:
                        binding[v['name']]=t.var(f'p{len(args)}');args.append(v);sorts.append(types[v['name']])
        body=[t.remap_fact(f,binding) for f in facts]
        key=json.dumps([body,sorts],sort_keys=True)
        choices.append((key,body,args,sorts))
    return min(choices,key=lambda x:x[0]) if choices else None


def load_cells(path,sig):
    result=[]
    for p in sorted((path/'catalog/states').glob('*.json')):
        state=json.loads(p.read_text());scope=state.get('scope',{})
        if scope.get('semantics') not in ('positive-constructor-equality/v1','positive-constructor-table-equality/v1') or scope.get('symbolic_size_conditions'):continue
        rows=[r for i,r in enumerate(state['rows']) if i not in state.get('subsumed_rows',[]) and r['op'] in sig and sig[r['op']]['kind']=='constructor']
        result.append((p.name,Facts(dict(state,rows=rows,subsumed_rows=[]))))
    return result


def witnesses(body,types,cells):
    hits=[]
    for name,index in cells:
        if any(not index.by_op[f['args'][1]['head']] for f in body):continue
        for b,_ in index.full(body):
            if all(index.state['values'][b[v]]['sort']==sort for v,sort in types.items()):hits.append(name);break
    return hits


def publications(rule,views,sig):
    """Compile only connections among observations in this application.

    LHS rows and constructed RHS rows are known. Equality closure includes
    committed RHS unions; arbitrary mutable function outputs are never asserted.
    """
    expressions=[e for f in rule['body'] for e in (f['args'] if f['kind']=='eq' else [f['expr']])]
    outputs=[e for a in rule['head'] for e in ([a['expr']] if a['kind'] in ('expr','let') else a.get('args',[])+([a['value']] if a['kind']=='set' else []))]
    expressions += outputs
    if any(v['kind']=='var' and v['name']=='_' for e in expressions for v in walk(e)):return []
    ops={e['head'] for root in expressions for e in walk(root) if e['kind']=='call'}
    created_ops={e['head'] for root in outputs for e in walk(root) if e['kind']=='call'}
    views=[v for v in views if all(f['args'][1]['head'] in ops for f in v['body']) and
           any(f['args'][1]['head'] in created_ops for f in v['body'])]
    if not views:return []
    cg=Congruence();observed=[]
    def pure(e):return all(x['kind']!='call' or sig.get(x['head'],{}).get('kind')=='constructor' for x in walk(e))
    def observe(e):
        for sub in walk(e):
            if sub['kind']=='call' and sig.get(sub['head'],{}).get('kind')=='constructor' and pure(sub):
                cg.add(sub);observed.append(sub)
        return e
    for f in rule['body']:
        es=[observe(e) for e in (f['args'] if f['kind']=='eq' else [f['expr']])]
        if f['kind']=='eq':cg.union(*es)
    for a in rule['head']:
        if a['kind']=='let':
            e=observe(a['expr']);cg.add(t.var(a['name']))
            # A second function read after set is a different observation.
            # Preserve the let-bound value, not an equality to a timeless call.
            if pure(e):cg.union(t.var(a['name']),e)
        elif a['kind']=='union':
            es=[observe(e) for e in a['args']]
            if all(pure(e) for e in es):cg.union(*es)
        elif a['kind']=='expr':observe(a['expr'])
        elif a['kind']=='set':
            for e in a['args']+[a['value']]:observe(e)
    cg.close();rows=defaultdict(list)
    # Keep native let-bound DAG identities. Expanding chained lets into terms
    # can turn a small action DAG into an exponential syntax tree.
    representatives={}
    for key,e in cg.terms.items():
        if e['kind']=='var':representatives.setdefault(cg.find(key),e)
    def coordinate(e):return representatives.get(cg.find(cg.add(e)),e)
    unique_rows=defaultdict(dict)
    for e in observed:
        row=[coordinate(a) for a in e['args']+[e]]
        unique_rows[e['head']][tuple(cg.find(cg.add(a)) for a in row)]=row
    rows={op:list(rs.values()) for op,rs in unique_rows.items()}
    keys={op:{tuple(cg.find(cg.add(x)) for x in row[:-1]):row for row in rs} for op,rs in rows.items()}
    def same(a,b):return cg.find(cg.add(a))==cg.find(cg.add(b))
    out=[]
    for view in views:
        partial=[{}]
        for depth,f in enumerate(view['body']):
            root,call=f['args'];following=[]
            for old in partial:
                selected_rows=rows.get(call['head'],[])
                if depth:
                    key=tuple(cg.find(cg.add(old[a['name']] if a['kind']=='var' else a)) for a in call['args'])
                    found=keys.get(call['head'],{}).get(key)
                    selected_rows=[] if found is None else [found]
                for row in selected_rows:
                    b=dict(old);ok=True
                    for pat,val in zip(call['args']+[root],row):
                        if pat['kind']=='var':
                            if pat['name'] in b and not same(b[pat['name']],val):ok=False;break
                            b[pat['name']]=val
                        elif not same(pat,val):ok=False;break
                    if ok:following.append(b)
            partial=list({tuple(sorted((k,cg.find(cg.add(v))) for k,v in b.items())):b for b in following}.values())
        for b in partial:
            args=[b[f'p{i}'] for i in range(len(view['sorts']))]
            # Re-evaluating a function/primitive after an earlier set could
            # observe a different value and even materialize a false row.
            if any(e['kind']=='call' and sig.get(e['head'],{}).get('kind')!='constructor'
                   for arg in args for e in walk(arg)):continue
            out.append(dict(kind='set',head=view['name'],args=args,value=dict(kind='literal',sort='Unit',text='()')))
    return list({(a['head'],tuple(map(text,a['args']))):a for a in out}.values())


def schedule(s):
    k=s['kind']
    if k=='run':return f'(seq (run {REPAIR}) {s["text"]})'
    if k=='seq':return '(seq '+' '.join(schedule(x) for x in s['children'])+')'
    if k=='repeat':return f'(repeat {s["count"]} {schedule(s["child"])})'
    return f'(saturate {schedule(s["child"])})'


def compile_source(source,analysis,out):
    out.mkdir(parents=True,exist_ok=False)
    data=compiler.ast(source,out/'source-ast.json');sig=data['signatures']
    if any(PREFIX in c['text'] or REPAIR in c['text'] or SHADOW in c['text'] for c in data['commands']):raise ValueError('reserved marker namespace')
    if data['rewrites']:raise ValueError('this adapter expects native expanded Rule commands; rewrites must be desugared first')
    if any(c['text'].startswith('(include ') for c in data['commands']):raise ValueError('expand includes before marker compilation')
    if any(c['text'].startswith(('(push','(pop')) for c in data['commands']):raise ValueError('push/pop scopes are not supported by this flat-program adapter')
    active=set()
    def sets(s):
        if s['kind']=='run':active.add(s['ruleset'])
        for x in s.get('children',[]):sets(x)
        if 'child' in s:sets(s['child'])
    for c in data['commands']:
        if c['kind']=='schedule':sets(c['schedule'])
    first_run=min(c['ordinal'] for c in data['commands'] if c['kind']=='schedule')
    if any(r['command_ordinal']>first_run for r in data['rules']):raise ValueError('define all rules before the first Run for this adapter')
    install=max(r['command_ordinal'] for r in data['rules'])+1
    while True:
        more={x for name in active for x in data.get('combined_rulesets',{}).get(name,[])}-active
        if not more:break
        active.update(more)
    # A permanent positive marker may only certify immutable visible rows.
    changed={a['head'] for r in data['rules'] for a in r['head'] if a['kind']=='change'}
    changed.update(a['action']['head'] for a in data['actions'] if a['action']['kind']=='change')
    cells=load_cells(analysis,sig);groups={};flats={}
    for rule in data['rules']:
        if rule['ruleset'] not in active:continue
        globals_={a['action']['name'] for a in data['actions'] if a['action']['kind']=='let' and a['command_ordinal']<rule['command_ordinal']}
        flat=flatten(rule,sig,globals_)
        if flat is None:continue
        rid=rule['ordinal'];flats[rid]=flat
        for i,j in itertools.combinations(range(flat['atom_count']),2):
            pair=[flat['body'][i],flat['body'][j]]
            if any(f['args'][1]['head'] in changed for f in pair):continue
            if not set.union(*(vars_in(e) for e in pair[0]['args'])) & set.union(*(vars_in(e) for e in pair[1]['args'])):continue
            normalized=canonical(pair,flat['types'])
            if normalized is None:continue
            key,body,args,sorts=normalized
            g=groups.setdefault(key,dict(body=body,sorts=sorts,occurrences=[]))
            g['occurrences'].append(dict(rule=rid,atoms=[i,j],args=args))
    eligible=[]
    for g in groups.values():
        if len({o['rule'] for o in g['occurrences']})<2:continue
        g['cells']=witnesses(g['body'],{f'p{i}':s for i,s in enumerate(g['sorts'])},cells)
        if g['cells']:eligible.append(g)
    # Exact non-overlapping schema cover. This score selects shared fragments;
    # measured Pass-3 work remains the acceptance test, not a fabricated model.
    solver.select('scip')
    with solver.Model('pass3_spine_cover') as model:
        vs=[];uses=defaultdict(list);chosen=[]
        for i,g in enumerate(eligible):
            v=model.addVar(vtype=solver.BINARY,name=f'view_{i}');vs.append(v)
            os=[]
            for n,o in enumerate(g['occurrences']):
                u=model.addVar(vtype=solver.BINARY,name=f'use_{i}_{n}');model.addConstr(u<=v);os.append(u)
                for a in o['atoms']:uses[o['rule'],a].append(u)
            model.addConstr(solver.quicksum(os)>=2*v);chosen.append(os)
        for xs in uses.values():model.addConstr(solver.quicksum(xs)<=1)
        coverage=solver.quicksum(u for xs in chosen for u in xs)
        objective=2*solver.quicksum(vs)-coverage
        model.setObjective(objective)
        model.optimize()
        if model.Status!=solver.OPTIMAL:raise ValueError('marker cover not solved')
        primary=model.ObjVal
        model.addConstr(objective==primary)
        model.setObjective(-coverage);model.optimize()
        if model.Status!=solver.OPTIMAL:raise ValueError('marker cover tie-break not solved')
        views=[];by_rule=defaultdict(list)
        for i,g in enumerate(eligible):
            if model.value(vs[i])<.5:continue
            view=dict(g,name=f'{PREFIX}{len(views)}');views.append(view)
            view['occurrences']=[o for o,u in zip(g['occurrences'],chosen[i]) if model.value(u)>.5]
            for o in view['occurrences']:by_rule[o['rule']].append((view,o))
        model.write(out/'selection.lp');model.write(out/'selection.sol')
    original={r['ordinal']:r for r in data['rules']};variants={}
    for mode in ('shared','published'):
        replacements={};pub_count=0
        for rid,rule in original.items():
            selected=by_rule.get(rid,[]);r=flats[rid] if selected else rule
            body=list(r['body']);head=list(r['head'])
            if selected:
                removed={a for _,o in selected for a in o['atoms']}
                body=[f for i,f in enumerate(body) if i not in removed]
                body += [dict(kind='fact',expr=t.call(v['name'],o['args'])) for v,o in selected]
            pubs=publications(r,views,sig) if mode=='published' and rule['ruleset'] in active else []
            pub_count+=len(pubs);head+=pubs
            if selected or pubs:
                # Keep the original rule name and original actions/naive flag.
                late_globals={a['action']['name'] for a in data['actions'] if a['action']['kind']=='let'
                              and rule['command_ordinal']<a['command_ordinal']<install}
                locals_used=set().union(set(),*(vars_in(e) for f in body for e in (f['args'] if f['kind']=='eq' else [f['expr']])))
                if late_globals & locals_used:raise ValueError('late marker installation would capture a rule variable as a global')
                replacements[r['command_ordinal']]=render_rule(r,body,head)
        # A Unit-valued view does not allocate a new equality sort/identity for
        # every tuple (this fork lowers `relation` to a constructor).
        declarations=[f'(ruleset {REPAIR})']+[f'(function {v["name"]} ({" ".join(v["sorts"])}) Unit :merge old)' for v in views]
        repair=[f'(rule ({" ".join(t.fact_text(f) for f in v["body"])}) ((set ({v["name"]} {" ".join(f"p{i}" for i in range(len(v["sorts"])))} ) ())) :ruleset {REPAIR} :name "complete-{v["name"]}")' for v in views]
        # Preserve original declaration/compilation order. Installing auxiliary
        # tables amid original declarations changes native table/rule identities.
        # Compile replaced rules in unscheduled private sets at their old sites;
        # install their executable variants and views immediately before Run.
        insert=min(replacements) if replacements else None;lines=[]
        by_command={r['command_ordinal']:r for r in original.values()}
        for c in data['commands']:
            if c['ordinal']==insert:
                lines += [f'(ruleset {SHADOW}{i})' for i in sorted(replacements)]
            if c['ordinal']==install and replacements:
                lines+=declarations+repair+[replacements[i] for i in sorted(replacements)]
            if c['kind']=='schedule' and views:lines.append('(run-schedule '+schedule(c['schedule'])+')')
            elif c['ordinal'] in replacements:
                r=by_command[c['ordinal']];lines.append(render_rule(r,r['body'],r['head'],f'{SHADOW}{c["ordinal"]}'))
            else:lines.append(c['text'])
        dest=out/f'{mode}.egg';dest.write_text('\n'.join(lines)+'\n')
        variants[mode]=dict(path=str(dest),publish_actions=pub_count,changed_rules=len(replacements),sha256=compiler.digest(dest))
    library_files=[analysis/'catalog/catalog.json']+sorted((analysis/'catalog/states').glob('*.json'))
    report=dict(schema='pass3-source-markers/v1',source=str(source),source_sha256=compiler.digest(source),analysis=str(analysis),
        analysis_sha256={str(p.relative_to(analysis)):compiler.digest(p) for p in library_files},
        cells=[name for name,_ in cells],markers=views,variants=variants,modified_consumers=len(by_rule),
        scheduled_rulesets=sorted(active),
        structural_objective=primary,tie_break='maximum shared occurrences at the same structural objective',
        rejected_tables=sorted(changed),solver=solver.statistics(),
        guarantee='exact positive constructor views completed before every original Run; original mutable reads/guards/RHS retained; no Cell closed-world assumption',
        boundary='auxiliary writes and changed enumeration may affect nonconfluent saturation; full native graph and extraction checks are required before adoption',
        selection_scope='non-overlapping fully-keyed two-atom shared fragments in scheduled rules, witnessed in Analyze; structural schema cover, not native-work global optimum')
    (out/'markers.json').write_text(json.dumps(report,indent=2)+'\n');return report


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',required=True,type=Path);p.add_argument('--analysis',required=True,type=Path);p.add_argument('--output',required=True,type=Path)
    a=p.parse_args();r=compile_source(a.source.resolve(),a.analysis.resolve(),a.output.resolve())
    print(json.dumps({k:r[k] for k in ('modified_consumers','variants','selection_scope')}))

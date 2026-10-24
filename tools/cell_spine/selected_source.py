"""Lower the selected typed spine IR to executable .egg; never run a solver.

Positive constructor prefixes are materialized as a shared DAG. An existing
root table is its own marker. Omitted payload fields use a correlated receipt,
not independent projections. Mutable/deleted prefix sources fail closed.
"""
import argparse
import copy
import json
from pathlib import Path
from . import compiler, shared_prefix, pass3_markers as old, takeover as t
from .terms import actions, text, walk, egg_string

PREFIX='CellSelectedSpine'
RECEIPT='CellSelectedReceipt'
BUILD='cell-selected-spines'
SHADOW='cell-selected-original'

def active_sets(data):
    active=set()
    def scan(s):
        if s['kind']=='run':active.add(s['ruleset'])
        for c in s.get('children',[]):scan(c)
        if 'child' in s:scan(s['child'])
    for c in data['commands']:
        if c['kind']=='schedule':scan(c['schedule'])
    while True:
        more={x for n in active for x in data.get('combined_rulesets',{}).get(n,[])}-active
        if not more:return active
        active|=more

def op(o,names):return names[o['Slot']] if 'Slot' in o else o['Literal']
def atom(a,names,globals_=frozenset()):
    cols=a['columns'];call=f'({a["table"]} '+ ' '.join(op(c,names) for c in cols[:-1])+')'
    if a['table'] in globals_ and len(cols)==1:call=a['table']
    return f'(= {op(cols[-1],names)} {call})'

def source_binding(rule,shape,sig,globals_=frozenset()):
    # Use the native table schema to flatten source expressions; primitives
    # remain residuals. This is an offline alpha alignment, not a runtime Join.
    tables={a['table']:dict(kind='constructor',input=a['sorts'][:-1],output=a['sorts'][-1]) for a in shape['atoms']}
    flat=old.flatten(rule,{**sig,**tables},globals_)
    if flat is None:raise ValueError('selected source wildcard must be normalized first')
    fs=flat['body'][:flat['atom_count']]
    native=[a for a in shape['atoms'] if a['table'] not in globals_]
    if len(fs)!=len(native):raise ValueError('surface/native table atoms differ')
    anchor={n:dict(Slot=i) for i,(n,_) in enumerate(shape['slots']) if n in flat['types']}
    anchor.update({a['table']:a['columns'][-1] for a in shape['atoms'] if a['table'] in globals_})
    def match(f,a,b):
        result,call=f['args']
        if call['head']!=a['table'] or len(call['args'])+1!=len(a['columns']):return None
        b=dict(b)
        for e,c,s in zip(call['args']+[result],a['columns'],a['sorts']):
            if e['kind']=='var':
                n=e['name']
                if flat['types'].get(n)!=s or (n in b and b[n]!=c):return None
                b[n]=c
            elif e['kind']!='literal' or c.get('Literal')!=e['text']:return None
        return b
    def search(pending,available,b):
        if not pending:return b
        choices=[(i,[(j,m) for j in available if (m:=match(fs[i],native[j],b)) is not None]) for i in pending]
        i,options=min(choices,key=lambda x:len(x[1]))
        for j,m in options:
            found=search(pending-{i},available-{j},m)
            if found is not None:return found
        return None
    binding=search(set(range(len(fs))),set(range(len(fs))),anchor)
    if binding is None:raise ValueError('cannot align complete native query to source RHS')
    return flat,binding

def definitions(specs):
    defs={};uses=[]
    for qi,spec in enumerate(specs):
        plan=spec['plan'];parent=None
        for depth in range(plan['prefix_len']+1):
            p=copy.deepcopy(plan);p['prefix_len']=depth
            if depth<plan['prefix_len']:p['carried_slots']=list(range(len(plan['shape']['slots'])))
            d=shared_prefix.describe(p)
            key=(None if plan.get('shared_marker') else qi,d['key'])
            if key not in defs:defs[key]=dict(definition=d,parent_key=parent)
            elif defs[key]['parent_key']!=parent:raise ValueError('inconsistent shared-prefix parent')
            parent=key
        uses.append(dict(spec=spec,marker_key=parent,coordinates=shared_prefix.describe(plan)['slots']))
    ordered=sorted(defs,key=lambda k:json.dumps(k,separators=(',',':')))
    ids={k:i for i,k in enumerate(ordered)};markers=[]
    for key in ordered:
        m=defs[key];d=m['definition'];types={c['Slot']:c['sort'] for a in d['body']['atoms'] for c in a['columns'] if 'Slot' in c}
        missing=sorted(set(types)-set(d['carried']))
        markers.append(dict(id=ids[key],definition=d,sorts=[types[i] for i in range(len(types))],missing=missing,
            parent=ids[m['parent_key']] if m['parent_key'] is not None else None,
            alias=len(d['body']['atoms'])==1 and not missing))
    for u in uses:u['marker']=ids[u.pop('marker_key')]
    return markers,uses

def read_marker(m,values,tag='w'):
    if m['alias']:return [atom(m['definition']['body']['atoms'][0],values)]
    args=[values[i] for i in m['definition']['carried']]
    if m['missing']:
        return [f'({PREFIX}{m["id"]} '+ ' '.join(args+[tag])+')',
                f'(= {tag} ({RECEIPT}{m["id"]} '+ ' '.join(values[i] for i in m['missing'])+'))']
    return [f'({PREFIX}{m["id"]} '+ ' '.join(args)+')']

def publish(m,values):
    args=[values[i] for i in m['definition']['carried']]
    if m['missing']:args.append(f'({RECEIPT}{m["id"]} '+ ' '.join(values[i] for i in m['missing'])+')')
    return f'(set ({PREFIX}{m["id"]} '+ ' '.join(args)+') ())'

def normalize_actions(head,prefix):
    """Expose actual RHS values, evaluating each original call exactly once.

    Do not identify a mutable/primitive read with a timeless expression. Its
    let-bound result can safely be an operand of a newly constructed spine row.
    """
    result=[];serial=0
    def visit(e):
        nonlocal serial
        if e['kind']!='call':return e
        call=dict(e,args=[visit(a) for a in e['args']]);name=f'{prefix}{serial}';serial+=1
        result.append(dict(kind='let',name=name,expr=call));return t.var(name)
    for action in head:
        a=copy.deepcopy(action);kind=a['kind']
        if kind in ('let','expr'):
            e=a['expr']
            if e['kind']=='call':e=dict(e,args=[visit(x) for x in e['args']])
            if kind=='expr' and e['kind']=='call':
                a=dict(kind='let',name=f'{prefix}{serial}',expr=e);serial+=1
            else:a['expr']=e
        elif kind=='union':a['args']=[visit(x) for x in a['args']]
        elif kind=='set':a['args']=[visit(x) for x in a['args']];a['value']=visit(a['value'])
        result.append(a)
    return result


def lower(data,specs,protected=()):
    if data['rewrites']:raise ValueError('selected source backend requires expanded rules')
    if any(c['text'].startswith(('(include ','(push','(pop','(input ')) for c in data['commands']):raise ValueError('requires flat self-contained source without scopes/input')
    if any(any(p in c['text'] for p in [PREFIX,RECEIPT,BUILD,SHADOW,'cell_selected_']) for c in data['commands']):raise ValueError('reserved selected-marker namespace')
    data=copy.deepcopy(data)
    commands={c['ordinal']:c['text'] for c in data['commands']}
    for rule in data['rules']:rule['name']=rule['name'] or commands[rule['command_ordinal']]
    active=active_sets(data);sig=data['signatures']
    if any(r.get('naive') and r['ruleset'] in active for r in data['rules']):raise ValueError('extra view progress is not certified for active naive rules')
    changed={a['head'] for r in data['rules'] if r['ruleset'] in active for a in r['head'] if a['kind']=='change'}
    changed|={x['action']['head'] for x in data['actions'] if x['action']['kind']=='change'}
    globals_={x['action']['name'] for x in data['actions'] if x['action']['kind']=='let'}
    lowered_specs=[];retained=[]
    for spec in specs:
        spec=copy.deepcopy(spec);p=spec['plan'];shape=p['shape']
        if not p.get('shared_marker') and p['prefix_len']==0:
            retained.append(dict(rule=spec['rule'],ruleset=spec['ruleset'],reason='private query plan selects no materialized source prefix'));continue
        ids=[p['seed']]+[i for i,_ in p['steps'][:p['prefix_len']]]
        stable=[i for i in ids if sig.get(shape['atoms'][i]['table'],{}).get('kind') in ('constructor','relation') and shape['atoms'][i]['table'] not in changed]
        if p['seed'] not in stable:
            retained.append(dict(rule=spec['rule'],ruleset=spec['ruleset'],reason='mutable/subsumable source anchor; original consumer retained'));continue
        before=p['steps']
        p['steps']=[x for x in before if x[0] in stable]+[x for x in before if x[0] not in stable]
        p['prefix_len']=len(stable)-1
        available={o['Slot'] for i in stable for o in shape['atoms'][i]['columns'] if 'Slot' in o}
        p['carried_slots']=[i for i in p['carried_slots'] if i in available]
        d=shared_prefix.describe(p)
        if len(d['body']['atoms'])==1 and len(d['carried'])==len(d['slots']):
            retained.append(dict(rule=spec['rule'],ruleset=spec['ruleset'],reason='existing table already is the complete marker; preserve source consumer'));continue
        lowered_specs.append(spec)
    markers,uses=definitions(lowered_specs)
    for m in markers:
        for a in m['definition']['body']['atoms']:
            if sig.get(a['table'],{}).get('kind') not in ('constructor','relation') or a['table'] in changed:
                raise ValueError('permanent source marker needs immutable visible constructor facts: '+a['table'])
    rules={(r['ruleset'],r['name']):r for r in data['rules']}
    selected={};publisher_rules={};consumers=set();records=[]
    for qi,u in enumerate(uses):
        spec=u['spec'];key=(spec['ruleset'],spec['rule']);r=rules[key];p=spec['plan'];shape=p['shape']
        if shape.get('native_residuals') or shape.get('primitives'):raise ValueError('source lowering requires exact structural query plus numeric guards')
        flat,binding=source_binding(r,shape,sig,globals_)
        names=[next((n for n,v in binding.items() if v==dict(Slot=i)),f'cell_selected_q{qi}_s{i:04d}') for i in range(len(shape['slots']))]
        u['source_names']=names
        coordinates=[names[i] for i in u['coordinates']]
        body=read_marker(markers[u['marker']],coordinates,f'cell_selected_q{qi}_receipt')
        covered={p['seed']}|{i for i,_ in p['steps'][:p['prefix_len']]}
        body += [atom(a,names,globals_) for i,a in enumerate(shape['atoms']) if i not in covered]
        for code,operands in shape['guards']:
            body.append(f'({["<",">","<=",">="][code]} '+ ' '.join(op(c,names) for c in operands)+')')
        subst={n:(t.var(names[v['Slot']]) if 'Slot' in v else dict(kind='literal',text=v['Literal'])) for n,v in binding.items()}
        head=[a if a['kind']=='panic' else t.remap_action(a,subst) for a in flat['head']]
        head=normalize_actions(head,f'cell_selected_q{qi}_a')
        selected[r['command_ordinal']]=(body,head)
        publisher_rules[r['command_ordinal']]=dict(r,body=[t.remap_fact(f,subst) for f in flat['body']],head=head)
        consumers.add(r['ruleset']);records.append(dict(ruleset=r['ruleset'],rule=r['name'],marker=u['marker'],
            plan=p,coordinates=u['coordinates'],prefix_atoms=len(covered),residual_atoms=len(shape['atoms'])-len(covered)))
    # Parent markers retain all fields; terminal projections may instead use
    # one receipt constructor for the omitted correlated coordinates.
    declarations=[f'(ruleset {BUILD})'];builders=[];views=[]
    for m in markers:
        i=m['id'];d=m['definition'];values=[f'cell_selected_p{j}' for j in range(len(m['sorts']))]
        declarations.append('; @selected-marker '+json.dumps(m,separators=(',',':')))
        if m['alias']:continue
        extra=[]
        if m['missing']:
            sort=f'{RECEIPT}Sort{i}';extra=[sort]
            declarations.append(f'(datatype {sort} ({RECEIPT}{i} '+ ' '.join(m['sorts'][j] for j in m['missing'])+'))')
        declarations.append(f'(function {PREFIX}{i} ('+' '.join([m['sorts'][j] for j in d['carried']]+extra)+') Unit :merge old)')
        body=([atom(d['body']['atoms'][0],values)] if m['parent'] is None else
              read_marker(markers[m['parent']],values,f'cell_selected_parent{i}')+[atom(d['body']['atoms'][-1],values)])
        builders.append(f'(rule ({" ".join(body)}) ({publish(m,values)}) :ruleset {BUILD} :name "build-selected-{i}")')
        views.append(dict(name=f'{PREFIX}{i}',sorts=m['sorts'],body=[dict(kind='eq',args=[
            t.var(f'p{a["columns"][-1]["Slot"]}') if 'Slot' in a['columns'][-1] else dict(kind='literal',text=a['columns'][-1]['Literal']),
            t.call(a['table'],[t.var(f'p{c["Slot"]}') if 'Slot' in c else dict(kind='literal',text=c['Literal']) for c in a['columns'][:-1]])]) for a in d['body']['atoms']]))
    replacements={};publish_count=0
    for r in data['rules']:
        body,head=selected.get(r['command_ordinal'],(None,normalize_actions(r['head'],f'cell_selected_r{r["command_ordinal"]}_a')))
        observed=publisher_rules.get(r['command_ordinal'],dict(r,head=head))
        pubs=old.publications(observed,views,sig,allow_partial_keys=True) if r['ruleset'] in active and r['ruleset'] not in protected and (r['ruleset'],r['name']) not in protected else []
        publication=[]
        for a in pubs:
            m=next(m for m in markers if f'{PREFIX}{m["id"]}'==a['head'])
            emitted=publish(m,[text(e) for e in a['args']])
            # An already consumed prefix needs no redundant self-publication.
            old_marker=next((u for u in uses if (u['spec']['ruleset'],u['spec']['rule'])==(r['ruleset'],r['name'])),None)
            if old_marker is not None:
                coords=[old_marker['source_names'][i] for i in old_marker['coordinates']]
                if emitted==publish(markers[old_marker['marker']],coords):continue
            publication.append(emitted)
        if body is not None or publication:
            lhs=' '.join(body) if body is not None else ' '.join(t.fact_text(f) for f in r['body'])
            rhs=[f'(panic {egg_string(a["message"])})' if a['kind']=='panic' else actions([a])[0] for a in head]+publication
            set_text=f' :ruleset {r["ruleset"]}' if r['ruleset'] else ''
            replacements[r['command_ordinal']]=f'(rule ({lhs}) ({" ".join(rhs)}){set_text} :name {egg_string(r["name"])})'
        publish_count+=len(publication)
    while True:
        more={n for n,cs in data.get('combined_rulesets',{}).items() if any(c in consumers for c in cs)}-consumers
        if not more:break
        consumers|=more
    def schedule(s):
        if s['kind']=='run':return f'(seq (saturate {BUILD}) {s["text"]})' if s['ruleset'] in consumers else s['text']
        if s['kind']=='seq':return '(seq '+' '.join(schedule(c) for c in s['children'])+')'
        return '('+s['kind']+' '+(str(s['count'])+' ' if s['kind']=='repeat' else '')+schedule(s['child'])+')'
    first_run=min(c['ordinal'] for c in data['commands'] if c['kind']=='schedule')
    if any(r['command_ordinal']>first_run for r in data['rules']):raise ValueError('declare rules before inference')
    # Ruleset membership is insertion ordered. Moving only rewritten rules to
    # the end changes native commit order (observable for :merge old/new).
    # Reinstall the entire affected ruleset, in its original source order.
    affected={r['ruleset'] for r in data['rules'] if r['command_ordinal'] in replacements}
    for r in data['rules']:
        if r['ruleset'] in affected and r['command_ordinal'] not in replacements:
            replacements[r['command_ordinal']]=old.render_rule(r,r['body'],r['head'])
    replacements=dict(sorted(replacements.items()))
    install=max(r['command_ordinal'] for r in data['rules'])+1;first=min(replacements) if replacements else install
    by_ordinal={r['command_ordinal']:r for r in data['rules']};lines=[]
    for c in data['commands']:
        n=c['ordinal']
        if n==first:lines += [f'(ruleset {SHADOW}{i})' for i in replacements]
        if n==install:lines+=declarations+builders+list(replacements.values())
        if n in replacements:
            r=by_ordinal[n];lines.append(old.render_rule(r,r['body'],r['head'],f'{SHADOW}{n}'))
        elif c['kind']=='schedule':lines.append('(run-schedule '+schedule(c['schedule'])+')')
        else:lines.append(c['text'])
    return '\n'.join(lines)+'\n',dict(marker_backend='selected typed spine IR; parameterized RHS publishers',retained_native=retained,markers=markers,lowered_queries=records,publish_actions=publish_count,
        protected_publisher_rulesets=sorted(protected),maintenance_before=sorted(consumers),helper_ruleset=BUILD,
        runtime_rule_count=0,solver_invoked=False)

def compile_source(source,layout,out,root=compiler.ROOT,joint=None):
    source=Path(source).resolve();layout=Path(layout).resolve();out=Path(out).resolve();out.mkdir(parents=True,exist_ok=False)
    native=root/'target/analyzer/release/examples/scheduled_spine';ast=root/'target/analyzer/release/examples/lhs_ast'
    compiler.run([ast,source,out/'ast.json'])
    data=json.loads((out/'ast.json').read_text())
    if joint is None:joint=False
    compiler.run([native,source,out/'rebound.json']+(['--joint'] if joint else [])+['--rebind',layout])
    compiler.run([native,source,out/'native.json']+(['--joint'] if joint else ['--closures']))
    specs=json.loads((out/'rebound.json').read_text())['rules']
    evidence=json.loads((out/'native.json').read_text());protected=set()
    if 'protected_queries' in evidence:protected={tuple(p) for p in evidence['protected_queries']}
    else:
        for d in (evidence.get('certificates') or {}).get('certificates',[]):
            if d['method']=='parameterized-selection-closure/v1':protected.update(d['rulesets'])
    protected_specs=[s for s in specs if (s['ruleset'],s['rule']) in protected]
    specs=[s for s in specs if (s['ruleset'],s['rule']) not in protected]
    # A materialized source view may deliver an old match at a new frontier.
    # Replaying it is observable for an ordered merge. Until that scheduling
    # equivalence is proved, keep the source rule and execute its declared
    # marker wiring with the ORIGINAL query watermark. No rule-name exception.
    ordered=[]
    for name,sig in data['signatures'].items():
        merge=sig.get('merge') or {}
        if sig.get('output')!='Unit' and merge.get('kind')=='var' and merge.get('name') in ('old','new'):
            ordered.append(name)
    if ordered:
        code=source.read_text()
        report=dict(marker_backend='declared runtime prefix markers; original incremental query identity',
            retained_native=[],markers=[],lowered_queries=[],publish_actions=0,
            protected_publisher_rulesets=sorted(protected),maintenance_before=[],helper_ruleset=None,
            runtime_rule_count=0,solver_invoked=False,
            source_lowering_obligation=dict(kind='ordered-merge-frontier',functions=sorted(ordered),
                decision='preserve source consumers; use selected runtime marker wiring'))
    else:code,report=lower(data,specs,protected)
    report['closure_owned_queries']=[dict(ruleset=s['ruleset'],rule=s['rule']) for s in protected_specs]
    headers=[s for s in source.read_text().splitlines() if s.startswith('; cell-structural-key-runtime/v1 ')]
    target=out/'source_sk_mk.egg'
    unchanged=not report['lowered_queries'] and not report['publish_actions']
    if unchanged:target.write_bytes(source.read_bytes())
    else:target.write_text('\n'.join(headers)+'\n'+'; cell-marker-tables/v1 '+json.dumps(dict(
        tables=[PREFIX+str(m['id']) for m in report['markers'] if not m['alias']]),separators=(',',':'))+'\n'+code)
    # Only rewritten consumers leave the runtime provider. Retained private or
    # mutable-root plans remain usable on their original LHS; do not manufacture
    # one-atom source markers that the solver never selected.
    rewritten={(q['ruleset'],q['rule']) for q in report['lowered_queries']}
    kept=[s for s in specs if (s['ruleset'],s['rule']) not in rewritten]
    runtime_layout=out/'retained-layout.json';runtime_layout.write_text(json.dumps(dict(schema='cell-boundary-programs/v2',setup=[],rules=kept)))
    compiler.run([native,target,out/'runtime-layout.json']+(['--joint'] if joint else [])+['--rebind',runtime_layout])
    sealed=out/'sealed.egg'
    compiler.run([native,target,sealed,'--closures','--seal',out/'runtime-layout.json'])
    sealed.replace(target)
    report.update(runtime_rule_count=len(kept),source_preserved=unchanged,inline_cell_declarations=True)
    report['runtime_marker_consumers']=sum(bool(s['plan'].get('shared_marker')) for s in kept)
    report.update(schema='selected-spine-source/v1',source_sha256=compiler.digest(source),output_sha256=compiler.digest(target),
        selected_layout_sha256=compiler.digest(layout),runtime_layout_sha256=compiler.digest(out/'runtime-layout.json'),
        source=str(source),output=str(target),selection_scope='same selected typed roots, prefixes, payloads and consumers; no two-atom reselection',
        guarantee='positive constructor prefix DAG; full correlated receipts; original guards and RHS; native validation still required')
    (out/'markers.json').write_text(json.dumps(report,indent=2)+'\n');return report

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--selected-layouts',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();compile_source(a.source,a.selected_layouts,a.output,joint=False)

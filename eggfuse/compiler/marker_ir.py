"""Marker Program: typed, correlated views and their exact source maintenance.

The device binds this contract to the actual typed marked program. A hash is a
provenance check, not a proof that an arbitrary closure is complete.
"""
import hashlib,json

SCHEMA='eggfuse-marker-program/v2'
def digest(value):return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':')).encode()).hexdigest()
def semantics(core):return {k:core[k] for k in ['tables','rulesets','schedule']}

def build(report,core,source_bytes,analysis_bytes):
    tables={t['name']:t for t in core['tables']};markers=[]
    for v in report['markers']:
        name=v['name'];table=tables[name]
        if [s['name'] for s in table['schema']]!=v['sorts']+['Unit']:raise ValueError('marker fields differ from compiled schema')
        consumers=[];publishers=[];repair=[]
        for rs in core['rulesets']:
            for rule in rs['rules']:
                reads=[a for a in rule['body'] if a['call'].get('table')==name]
                writes=[a for a in rule['head'] if a.get('call',{}).get('table')==name]
                if reads:consumers.append(dict(ruleset=rs['name'],rule=rule['name'],reads=reads))
                if writes:
                    target=repair if rs['name']=='cell-spine-complete' else publishers
                    target.append(dict(ruleset=rs['name'],rule=rule['name'],writes=writes))
        if not consumers or len(repair)!=1:raise ValueError('marker lacks consumer or unique completeness rule')
        markers.append(dict(name=name,fields=[dict(slot=i,sort=s,variable=v['parameters'][i]) for i,s in enumerate(v['sorts'])],
            guarantee=v['body'],witness_cells=v['cells'],layout='full correlated tuple',
            consumers=consumers,publishers=publishers,completion=repair,
            maintenance=dict(initial='complete before first consumer',insert='committed RHS publication plus completion',
                union='canonicalize complete tuple and complete newly enabled bindings',
                removal='source view tables are statically excluded if subsumable/deletable',
                ports='mutable functions and numeric guards remain in consumer LHS'),
            effects='original consumer RHS; no closure endpoint substitution'))
    continuations=continuation_contracts(core,markers,[s['table'] for s in report.get('spine_states',[])])
    result=dict(schema=SCHEMA,source_sha256=hashlib.sha256(source_bytes).hexdigest(),
        analysis_sha256=hashlib.sha256(analysis_bytes).hexdigest(),typed_program=semantics(core),
        markers=markers,continuations=continuations,state_representation='prefix presence + live original ports + successful application receipts; original control owns stage and repeat',schedule=core['schedule'],cost_model=report['cost_model'],
        selection_scope=report['selection_scope'],closure_authority='none; complete positive source views only',
        obligations=['SK versus marked native projection','marked native versus GPU','original visible stage boundaries'])
    result['sha256']=digest(result);validate(result,core);return result

def validate(ir,core):
    if ir.get('schema') not in (SCHEMA,'eggfuse-marker-program/v1'):raise ValueError('unsupported marker IR schema')
    payload=dict(ir);claimed=payload.pop('sha256',None)
    if digest(payload)!=claimed:raise ValueError('marker program integrity mismatch')
    if ir['typed_program']!=semantics(core):raise ValueError('marker program no longer describes current typed rules/merge/schedule')
    tables={t['name']:t for t in core['tables']};names=set()
    for m in ir['markers']:
        name=m['name']
        if name in names:raise ValueError('duplicate marker');
        names.add(name);t=tables[name]
        if [f['slot'] for f in m['fields']]!=list(range(len(m['fields']))) or len({f['variable'] for f in m['fields']})!=len(m['fields']):raise ValueError('invalid marker coordinate map')
        if t['kind']!='function' or t['schema'][-1]['name']!='Unit':raise ValueError('marker must not allocate equality identities')
        if t['merge']!='old':raise ValueError('marker merge contract changed')
        if [s['name'] for s in t['schema'][:-1]]!=[s['sort'] for s in m['fields']]:raise ValueError('marker ABI changed')
        actual_reads=[];actual_writes=[]
        for rs in core['rulesets']:
            for r in rs['rules']:
                reads=[a for a in r['body'] if a['call'].get('table')==name]
                writes=[a for a in r['head'] if a.get('call',{}).get('table')==name]
                if reads:actual_reads.append(dict(ruleset=rs['name'],rule=r['name'],reads=reads))
                if writes:actual_writes.append(dict(ruleset=rs['name'],rule=r['name'],writes=writes))
        if actual_reads!=m['consumers']:raise ValueError('marker consumer contract differs')
        expected=m['completion']+m['publishers']
        if sorted(map(digest,actual_writes))!=sorted(map(digest,expected)):raise ValueError('marker publisher contract differs')
        if len(m['completion'])!=1:raise ValueError('one exact completion rule required')
        proof=m['completion'][0]
        rule=next(r for s in core['rulesets'] for r in s['rules'] if s['name']==proof['ruleset'] and r['name']==proof['rule'])
        copies={a['dst']:a['arg'] for a in rule['head'] if a['op']=='copy'}
        def resolve(x):
            seen=set()
            while x.get('var') in copies:
                if x['var'] in seen:raise ValueError('cyclic marker copies')
                seen.add(x['var']);x=copies[x['var']]
            return x
        write=proof['writes'][0]
        coordinates={resolve(x)['var']:i for i,x in enumerate(write['args'])}
        if len(coordinates)!=len(write['args']):raise ValueError('marker fields lose a correlated coordinate')
        named={f['variable']:f['slot'] for f in m['fields']}
        def literal(sort,value):return ['literal',sort,int(value) if sort=='bool' else value]
        def operand(x,raw=False):
            if raw:
                if x['kind']=='var':return ['slot',named[x['name']]]
                if x['kind']=='literal':return literal(x['sort'],x['value'])
                raise ValueError('non-flat marker guarantee')
            if 'var' in x:return ['slot',coordinates[resolve(x)['var']]]
            return literal(x['sort']['name'],x['literal'])
        by_name={x['name']:x for x in ir['markers']}
        def expand(a):
            table=a['call']['table']
            if table not in by_name:return [[table,[operand(v) for v in a['args']]]]
            parent=by_name[table]
            # A shorter exact structural guarantee cannot manufacture a
            # cyclic proof. Its Unit return is not an equality identity.
            if len(parent['guarantee'])>=len(m['guarantee']):raise ValueError('cyclic marker prefix')
            binding={f['variable']:operand(a['args'][f['slot']]) for f in parent['fields']}
            def sub(v):
                return binding[v['name']] if v['kind']=='var' else literal(v['sort'],v['value'])
            return [[f['args'][1]['head'],[sub(v) for v in f['args'][1]['args']+[f['args'][0]]]] for f in parent['guarantee']]
        actual=sorted(digest(x) for a in rule['body'] for x in expand(a))
        required=sorted(digest([f['args'][1]['head'],[operand(v,True) for v in f['args'][1]['args']+[f['args'][0]]]]) for f in m['guarantee'])
        if actual!=required:raise ValueError('marker guarantee is not its exact completion query: '+name)
        inputs={f['args'][1]['head'] for f in m['guarantee']}
        if any(tables[n]['kind']!='constructor' for n in inputs):raise ValueError('permanent marker cannot certify mutable function rows')
        if any(a['op'] in ('subsume','delete') and a.get('call',{}).get('table') in inputs for rs in core['rulesets'] for r in rs['rules'] for a in r['head']):raise ValueError('marker input visibility can change')
        completion=proof['ruleset'];consumers={c['ruleset'] for c in m['consumers']}-{completion}
        def control(s,previous=None):
            if 'run' in s:
                if s['run'] in consumers and previous!=completion:raise ValueError('marker consumer lacks immediately preceding completion')
                return s['run']
            if 'sequence' in s:
                for child in s['sequence']:previous=control(child,previous)
                return previous
            child=s.get('body',s.get('saturate'))
            if child is None:raise ValueError('unknown marker control')
            last=control(child,None)
            if 'saturate' in s and last==completion:return completion
            return None
        control({'sequence':core['schedule']})
    states=ir.get('continuations',[])
    if states!=continuation_contracts(core,ir['markers'],[s['receipt_table'] for s in states]):raise ValueError('Spine continuation contract differs')
    return names


def continuation_contracts(core,markers,receipt_tables):
    """Derive resumable table state from the actual rewritten rules.

    Prefix rows are ready structural ports. Function/guard inputs remain live
    exact reads; receipts record successful effects, not cached function values.
    Original control and native table timestamps own activation/Repeat progress.
    """
    tables={t['name']:t for t in core['tables']};names={m['name'] for m in markers};result=[]
    hidden={a['call']['table'] for rs in core['rulesets'] for r in rs['rules'] for a in r['head'] if a['op'] in ('subsume','delete')}
    for table in receipt_tables:
        t=tables[table]
        if t['kind']!='function' or t['schema'][-1]['name']!='Unit' or t['merge']!='old':raise ValueError('invalid Spine receipt schema')
        sites=[(rs,r,a) for rs in core['rulesets'] for r in rs['rules'] for a in r['head'] if a.get('call',{}).get('table')==table]
        if not sites:raise ValueError('receipt has no publisher')
        # Combined rulesets may contain the same source rule.
        unique={}
        for rs,r,a in sites:unique.setdefault((r.get('source_set',rs['name']),r['name']),(r,a))
        if len(unique)!=1:raise ValueError('receipt mixes independent bindings')
        identity,(r,write)=next(iter(unique.items()))
        if write['op']!='set' or write['value']!={'literal':0,'sort':{'name':'Unit','kind':'Unit'}}:raise ValueError('invalid successful receipt publication')
        if r['head'][-1]!=write:raise ValueError('receipt precedes original effects')
        aliases={a['dst']:a['arg'] for a in r['head'] if a['op']=='copy'}
        def resolve(x):
            seen=set()
            while x.get('var') in aliases:
                if x['var'] in seen:raise ValueError('cyclic receipt alias')
                seen.add(x['var']);x=aliases[x['var']]
            return x
        keys=[resolve(x) for x in write['args']]
        bound={x['var'] for a in r['body'] for x in a['args'] if 'var' in x}
        # Parser-generated existential relation result IDs need not be stored
        # unless an effect observes them. Every effect-observable input must be
        # represented; guards/ports themselves remain in the exact query.
        required={resolve(x)['var'] for a in r['head'][:-1] if a['op']!='copy' for x in a.get('args',[])+([a['arg']] if 'arg' in a else [])+([a['value']] if 'value' in a else []) if 'var' in resolve(x)} & bound
        available={x.get('var') for x in keys}
        # Deterministic primitive intermediates (e.g. k+1) need not duplicate
        # their inputs in the persistent key. Mutable table results do.
        for _ in r['body']:
            before=len(available)
            for a in r['body']:
                if a['call'].get('primitive') not in ('+','-','*','/','%','min','max','<','>','<=','>=','!=','not','and','or','xor','=>','guard'):continue
                if all('literal' in x or x['var'] in available for x in a['args'][:-1]):available.add(a['args'][-1].get('var'))
            if len(available)==before:break
        effect_inputs_covered=required<=available
        original=[a for a in r['head'] if a is not write]
        safe=effect_inputs_covered and bool(original) and all(a['op']=='copy' or (a['op']=='let' and a.get('call',{}).get('table') in tables and tables[a['call']['table']]['kind']=='constructor' and tables[a['call']['table']].get('merge_expr') is None and a['call']['table'] not in hidden) for a in original)
        result.append(dict(receipt_table=table,identity=list(identity),keys=keys,
            structural_inputs=[a for a in r['body'] if a['call'].get('table') in names],
            ports=[a for a in r['body'] if a['call'].get('table') not in names],
            effects=original,
            completion='immutable-constructor-effects' if safe else 'historical-observation-only',
            activation='original Run membership, snapshot and per-rule semi-naive frontier; not a permanent done bit',
            readiness='structural prefix rows plus current correlated port rows; missing reads remain exact delta queries',
            storage='ordinary typed egraph tables; full Spine correlation, effect-input receipt projection; native canonicalization, visibility and timestamps'))
    return result

def completion_shortcuts(ir,core):
    """A functional immutable spine has at most one answer for its seed row.
    An existing full marker proves its own completion RHS is a no-op; it does
    not certify that an arbitrary eclass or surrounding Cell is complete.
    """
    validate(ir,core);result={}
    for m in ir['markers']:
        proof=m['completion'][0]
        rule=next(r for s in core['rulesets'] for r in s['rules'] if s['name']==proof['ruleset'] and r['name']==proof['rule'])
        if len(proof['writes'])!=1 or sum(a['op']=='set' for a in rule['head'])!=1 or any(a['op'] not in ('copy','set') for a in rule['head']):continue
        write=proof['writes'][0]
        if write['value'].get('literal')!=0 or write['value']['sort']['name']!='Unit':continue
        aliases={a['dst']:a['arg'] for a in rule['head'] if a['op']=='copy'}
        def resolve(x):
            while x.get('var') in aliases:x=aliases[x['var']]
            return x
        coords={f['variable']:resolve(write['args'][f['slot']]) for f in m['fields']}
        facts=m['guarantee'];seed=facts[0]
        available={v['name'] for v in seed['args'][1]['args']+[seed['args'][0]] if v['kind']=='var'}
        seed_names=set(available);functional=True
        for f in facts[1:]:
            if any(v['kind']=='var' and v['name'] not in available for v in f['args'][1]['args']):functional=False;break
            available|={v['name'] for v in f['args'][1]['args']+[f['args'][0]] if v['kind']=='var'}
        if not functional or not seed_names:continue
        slots=[dict(column=f['slot'],operand=coords[f['variable']]) for f in m['fields'] if f['variable'] in seed_names]
        result[(rule.get('source_set',proof['ruleset']),rule['name'])]=dict(table=m['name'],seed=slots)
    return result


def completed_view_keys(ir,core):
    """A complete view tuple proves its Unit insertion has already happened.

    Unlike a functional seed shortcut, this is valid for reverse fibers too:
    it checks the entire correlated key, never suppresses alternative tuples.
    """
    result={}
    for m in ir['markers']:
        proof=m['completion'][0]
        rule=next(r for rs in core['rulesets'] for r in rs['rules'] if rs['name']==proof['ruleset'] and r['name']==proof['rule'])
        if len(proof['writes'])!=1 or any(a['op'] not in ('copy','set') for a in rule['head']) or sum(a['op']=='set' for a in rule['head'])!=1:continue
        aliases={a['dst']:a['arg'] for a in rule['head'] if a['op']=='copy'}
        def resolve(x):
            while x.get('var') in aliases:x=aliases[x['var']]
            return x
        result[(rule.get('source_set',proof['ruleset']),rule['name'])]=dict(receipt_table=m['name'],keys=[resolve(x) for x in proof['writes'][0]['args']])
    return result

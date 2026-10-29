"""Marker Program: typed, correlated views and their exact source maintenance.

The device binds this contract to the actual typed marked program. A hash is a
provenance check, not a proof that an arbitrary closure is complete.
"""
import hashlib,json

SCHEMA='eggfuse-marker-program/v1'
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
    result=dict(schema=SCHEMA,source_sha256=hashlib.sha256(source_bytes).hexdigest(),
        analysis_sha256=hashlib.sha256(analysis_bytes).hexdigest(),typed_program=semantics(core),
        markers=markers,schedule=core['schedule'],cost_model=report['cost_model'],
        selection_scope=report['selection_scope'],closure_authority='none; complete positive source views only',
        obligations=['SK versus marked native projection','marked native versus GPU','original visible stage boundaries'])
    result['sha256']=digest(result);validate(result,core);return result

def validate(ir,core):
    if ir.get('schema')!=SCHEMA:raise ValueError('unsupported marker IR schema')
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
        def operand(x,raw=False):
            if raw:
                if x['kind']=='var':return ['slot',named[x['name']]]
                if x['kind']=='literal':return ['literal',x['sort'],x['value']]
                raise ValueError('non-flat marker guarantee')
            if 'var' in x:return ['slot',coordinates[resolve(x)['var']]]
            return ['literal',x['sort']['name'],x['literal']]
        actual=sorted(digest([a['call']['table'],[operand(v) for v in a['args']]]) for a in rule['body'])
        required=sorted(digest([f['args'][1]['head'],[operand(v,True) for v in f['args'][1]['args']+[f['args'][0]]]]) for f in m['guarantee'])
        if actual!=required:raise ValueError('marker guarantee is not its exact completion query')
        inputs={f['args'][1]['head'] for f in m['guarantee']}
        if any(tables[n]['kind']!='constructor' for n in inputs):raise ValueError('permanent marker cannot certify mutable function rows')
        if any(a['op'] in ('subsume','delete') and a.get('call',{}).get('table') in inputs for rs in core['rulesets'] for r in rs['rules'] for a in r['head']):raise ValueError('marker input visibility can change')
        consumers={c['ruleset'] for c in m['consumers']};completion=proof['ruleset']
        def control(s,previous=None):
            if 'run' in s:
                if s['run'] in consumers and previous!=completion:raise ValueError('marker consumer lacks immediately preceding completion')
                return s['run']
            if 'sequence' in s:
                for child in s['sequence']:previous=control(child,previous)
                return previous
            child=s.get('body',s.get('saturate'))
            if child is None:raise ValueError('unknown marker control')
            control(child,None);return None
        control({'sequence':core['schedule']})
    return names

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

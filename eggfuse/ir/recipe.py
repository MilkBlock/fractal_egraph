"""Typed, lossless Recipe IR shared by analysis adapters and device lowering.

A recipe describes execution, not permission to substitute an isolated endpoint
into a global graph. Dependency/event descriptors are conservative; residual
queries remain explicit until a separate optimization certificate discharges them.
"""
import copy
import hashlib
import json
from collections import defaultdict

SCHEMA = 'eggfuse-recipe/v1'
SCOPES = {'original_program', 'isolated_cell'}


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def environment(core):
    return {k:copy.deepcopy(v) for k,v in core.items() if k != 'initial'}


def _call(call, args, tables, result=None, query=False):
    require(('table' in call) != ('primitive' in call),'call must name exactly one table or primitive')
    if 'table' in call:
        require(call['table'] in tables, 'unknown recipe table')
        signature=tables[call['table']]['schema']
        expected=signature if query else signature[:-1]
        require(len(args)==len(expected), 'table arity differs')
        require(all(a['sort']==s for a,s in zip(args,expected)), 'table argument sort differs')
        if result is not None:require(result==signature[-1], 'table result sort differs')
    else:
        require('primitive' in call and 'inputs' in call and 'output' in call, 'invalid typed call')
        expected=call['inputs']+([call['output']] if query else [])
        require(len(args)==len(expected) and all(a['sort']['name']==s for a,s in zip(args,expected)), 'primitive signature differs')
        if result is not None:require(result['name']==call['output'], 'primitive result sort differs')


def _rule(rule, tables):
    slots=[];bindings={}
    def operand(value, bind=False):
        require(isinstance(value,dict) and 'sort' in value, 'untyped recipe operand')
        require(('var' in value) != ('literal' in value),'operand must be either variable or literal')
        if 'var' not in value:
            require('literal' in value, 'missing literal/variable')
            return copy.deepcopy(value)
        name=value['var']
        if name not in bindings:
            require(bind, 'effect reads an unbound variable: '+name)
            bindings[name]=len(slots);slots.append({'name':name,'sort':copy.deepcopy(value['sort']),'definition':'query'})
        slot=bindings[name]
        require(slots[slot]['sort']==value['sort'], 'variable sort changes')
        return {'slot':slot}
    query=[]
    for atom in rule['body']:
        _call(atom['call'],atom['args'],tables,query=True)
        query.append({**copy.deepcopy(atom),'args':[operand(a,True) for a in atom['args']]})
    effects=[]
    for action in rule['head']:
        op=action['op'];a=copy.deepcopy(action)
        require(op in ['copy','let','set','union','subsume','delete','panic'], 'unsupported recipe action '+op)
        if op=='copy':require(action['sort']==action['arg']['sort'], 'copy sort differs')
        if op=='let':_call(action['call'],action['args'],tables,action['sort'])
        if op in ['set','subsume','delete']:
            require('table' in action['call'],'mutation needs a table')
            _call(action['call'],action['args'],tables,action['value']['sort'] if op=='set' else None)
        if op=='union':
            require(len(action['args'])==2 and action['args'][0]['sort']==action['args'][1]['sort'] and action['args'][0]['sort']['kind']=='eq','union needs one equality sort')
        if 'args' in a:a['args']=[operand(v) for v in action['args']]
        for k in ['arg','value']:
            if k in a:a[k]=operand(action[k])
        if op in ['let','copy']:
            a['dst']=len(slots);bindings[action['dst']]=a['dst']
            slots.append({'name':action['dst'],'sort':copy.deepcopy(action['sort']),'definition':len(effects)})
        if op=='let':a['kind']='compute' if 'primitive' in a['call'] else ('construct' if tables[a['call']['table']]['kind']=='constructor' else 'read')
        else:a['kind']=op
        effects.append(a)
    inputs=[dict(atom=i,table=a['call']['table'],columns=copy.deepcopy(a['args']),key_columns=list(range(len(a['args'])-1)),witness='complete visible row') for i,a in enumerate(query) if 'table' in a['call']]
    return dict(metadata={k:copy.deepcopy(v) for k,v in rule.items() if k not in ['body','head']},slots=slots,
        query=query,effects=effects,interface=dict(rows=inputs,primitive_atoms=[i for i,a in enumerate(query) if 'primitive' in a['call']],
        guaranteed_atoms=[],residual_atoms=list(range(len(query))),correlation='one complete binding row; slot equality and literals remain constraints'))


def _footprint(rule,tables):
    reads={a['call']['table'] for a in rule['query'] if 'table' in a['call']};writes=set();unions=set();operations=set()
    for a in rule['effects']:
        operations.add(a['kind'])
        name=a.get('call',{}).get('table')
        if name:
            (reads if a['kind']=='read' else writes).add(name)
            if a['op']=='set' and tables[name]['kind']=='constructor':unions.add(tables[name]['schema'][-1]['name'])
        if a['op']=='union':unions.add(rule['slots'][a['args'][0]['slot']]['sort']['name'] if 'slot' in a['args'][0] else a['args'][0]['sort']['name'])
    pending=list(writes);seen=set()
    def merge(expr):
        if not isinstance(expr,dict):return
        name=expr.get('call',{}).get('table')
        if name:
            require(name in tables,'merge refers to unknown table')
            if tables[name]['kind']=='constructor':
                if name not in writes:writes.add(name);pending.append(name)
            else:reads.add(name)
        for a in expr.get('args',[]):merge(a)
    while pending:
        name=pending.pop()
        if name in seen:continue
        seen.add(name);merge(tables[name].get('merge_expr'))
    def touches(s):return s.get('name') in unions or (isinstance(s.get('element'),dict) and touches(s['element']))
    return dict(read_tables=sorted(reads),write_tables=sorted(writes),union_sorts=sorted(unions),
                equality_invalidates=sorted(t['name'] for t in tables.values() if any(touches(s) for s in t['schema'])),
                operations=sorted(operations),precision='conservative direct/merge footprint; global rebuild may have further effects')


def _control(schedule,sets):
    def node(s):
        if 'run' in s:
            require(s['run'] in sets,'unknown scheduled ruleset')
            if 'until_run' in s:require(s['until_run'] in sets,'unknown until probe')
            require(set(s)<= {'run','until_run','until','assert','assert_table'},'unknown Run control field')
        elif 'sequence' in s:
            require(set(s)=={'sequence'},'invalid sequence')
            for x in s['sequence']:node(x)
        elif 'repeat' in s:
            require(set(s)=={'repeat','body'} and type(s['repeat']) is int and s['repeat']>=0,'invalid Repeat')
            node(s['body'])
        elif 'saturate' in s:
            require(set(s)=={'saturate'},'invalid Saturate');node(s['saturate'])
        else:raise ValueError('unknown recipe control')
    for s in schedule:node(s)


def build(core,scope='original_program'):
    require(scope in SCOPES,'invalid execution scope')
    require(core.get('schema')=='egglog-device-core/v1','expected typed device core')
    tables={t['name']:t for t in core['tables']}
    require(len(tables)==len(core['tables']),'duplicate table')
    for t in tables.values():
        require(t['kind'] in ['constructor','function'] and t['schema'],'invalid table schema')
    names=[s['name'] for s in core['rulesets']];require(len(set(names))==len(names),'duplicate ruleset')
    _control(core['schedule'],set(names))
    rules=[];ids={};sets=[]
    for rs in core['rulesets']:
        uses=[]
        for r in rs['rules']:
            normalized=_rule(r,tables);normalized['footprint']=_footprint(normalized,tables)
            key=digest(normalized)
            if key not in ids:ids[key]=len(rules);rules.append(normalized)
            uses.append(ids[key])
        sets.append({**{k:copy.deepcopy(v) for k,v in rs.items() if k!='rules'},'rule_ids':uses})
    readers=defaultdict(list)
    for rid,r in enumerate(rules):
        for row in r['interface']['rows']:readers[row['table']].append((rid,row))
    publishers=defaultdict(list)
    for producer,r in enumerate(rules):
        for ei,e in enumerate(r['effects']):
            table=e.get('call',{}).get('table')
            if e['kind'] in ['construct','set'] and table in readers:
                publishers[table].append(dict(rule=producer,effect=ei))
    # One route per table, not one copy of all consumers per emitted row.
    events=[dict(table=table,publishers=producers,
        consumers=[dict(rule=rid,atom=row['atom'],columns=row['columns'],residual_atoms=[i for i in range(len(rules[rid]['query'])) if i!=row['atom']]) for rid,row in readers[table]],
        payload='actual post-merge canonical visible row; not the proposed set value',
        activation_requires=['same-row column constraints','residual query','original stage gate'],runtime_subscription_installed=False)
        for table,producers in sorted(publishers.items())]
    payload=dict(schema=SCHEMA,scope=scope,source_core_sha256=digest(environment(core)),
        core_attributes={k:copy.deepcopy(v) for k,v in core.items() if k not in ['initial','tables','rulesets','schedule']},
        tables=copy.deepcopy(core['tables']),rulesets=sets,rules=rules,control=copy.deepcopy(core['schedule']),event_routes=events,
        authorization=dict(live_substitution_authorized=False,guarantee='exact query/effect/control program; optimization needs a separately validated regional certificate'),
        capabilities=dict(operations=sorted({op for r in rules for op in r['footprint']['operations']}),
            generic_queries_retained=True,optimized_region=False))
    return {**payload,'sha256':digest(payload)}


def _decode(ir):
    rules=[]
    for r in ir['rules']:
        slots=r['slots']
        def operand(v):
            if 'slot' not in v:return copy.deepcopy(v)
            i=v['slot'];require(type(i) is int and 0<=i<len(slots),'invalid recipe slot')
            return {'var':slots[i]['name'],'sort':copy.deepcopy(slots[i]['sort'])}
        body=[{**copy.deepcopy(a),'args':[operand(v) for v in a['args']]} for a in r['query']]
        head=[]
        for e in r['effects']:
            a={k:copy.deepcopy(v) for k,v in e.items() if k!='kind'}
            for k in ['arg','value']:
                if k in a:a[k]=operand(a[k])
            if 'args' in a:a['args']=[operand(v) for v in a['args']]
            if 'dst' in a:
                i=a['dst'];require(type(i) is int and 0<=i<len(slots),'invalid effect destination');a['dst']=slots[i]['name']
            head.append(a)
        rules.append({**copy.deepcopy(r['metadata']),'body':body,'head':head})
    sets=[]
    for rs in ir['rulesets']:
        require(all(type(i) is int and 0<=i<len(rules) for i in rs['rule_ids']),'invalid rule reference')
        sets.append({**{k:copy.deepcopy(v) for k,v in rs.items() if k!='rule_ids'},'rules':[copy.deepcopy(rules[i]) for i in rs['rule_ids']]})
    return {**copy.deepcopy(ir['core_attributes']),'tables':copy.deepcopy(ir['tables']),'rulesets':sets,'schedule':copy.deepcopy(ir['control'])}


def validate(ir,expected=None,scope=None):
    require(ir.get('schema')==SCHEMA,'unsupported Recipe IR')
    require(ir.get('sha256')==digest({k:v for k,v in ir.items() if k!='sha256'}),'Recipe IR checksum differs')
    if scope is not None:require(ir['scope']==scope,'Recipe execution scope differs')
    core=_decode(ir)
    require(build(core,ir['scope'])==ir,'Recipe IR wiring/effects/control/derived contract differs')
    if expected is not None:require(core==environment(expected),'Recipe IR differs from typed source')
    return core


def instantiate(ir,initial,scope=None):
    return {**validate(ir,scope=scope),'initial':copy.deepcopy(initial)}

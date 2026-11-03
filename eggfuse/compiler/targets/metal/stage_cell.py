"""Bake/load the certified joint program and its stage context contracts.

Facts and concrete IDs are evidence, never part of the reusable template key.
An artifact cannot authorize a different rule environment or schedule.
"""
import hashlib,json
from pathlib import Path

def digest(x):
    return hashlib.sha256(json.dumps(x,sort_keys=True,separators=(',',':')).encode()).hexdigest()

def matching_spine(rule):
    """A separate slot namespace for each stage rule; preserve every join wire."""
    slots={};sorts={}
    def field(x):
        if 'var' not in x:return x
        name=x['var']
        if name not in slots:slots[name]=len(slots);sorts[name]=x.get('sort')
        return {'slot':slots[name]}
    atoms=[];guards=[]
    for atom in rule['body']:
        fields=[field(x) for x in atom['args']]
        if 'table' in atom['call']:atoms.append({'table':atom['call']['table'],'fields':fields})
        else:guards.append({'call':atom['call'],'fields':fields})
    return {'rule':rule['name'],'atoms':atoms,'guards':guards,
            'slots':[{'id':i,'sort':sorts[n]} for n,i in slots.items()],
            'correlation':'all atoms share this one binding row; never combine different rows or versions'}

def contract(core,joint):
    if joint is None:raise ValueError('multi-stage Cell requires a certified joint terms program')
    tables={t['name']:t for t in core['tables']}
    byname={s['name']:s for s in core['rulesets']}
    def calls(x):
        if isinstance(x,dict):
            if 'table' in x:yield x['table']
            for v in x.values():yield from calls(v)
        elif isinstance(x,list):
            for v in x:yield from calls(v)
    stages=[]
    for i,name in enumerate(joint['sets']):
        rules=byname[name]['rules'];reads=set();writes=set()
        for r in rules:
            reads.update(calls(r['body']));writes.update(calls(r['head']))
        pending=list(writes)
        while pending:
            name2=pending.pop()
            for t in calls(tables[name2].get('merge_expr')):
                if t not in writes:writes.add(t);pending.append(t)
        stages.append({'id':i,'ruleset':name,'operation':'saturate' if i==2 else 'run',
            'reads':sorted(reads),'writes':sorted(writes),
            'row_write_domain':['append','function_rows_and_append','mutable_rows'][i],
            'rules':rules,'matching_spines':[matching_spine(r) for r in rules],
            'equality_sorts':joint['comparison']['affected_sorts'] if i==2 else [],
            'input':'immutable version, including equality and watermarks',
            'output':'patch relative to exactly that input version'})
    value={'schema':'multi-stage-cell-contract/v1','schedule':joint['schedule'],
        'tables':sorted((t for t in core['tables'] if not t.get('internal') and t['name'] not in core.get('assertion_tables',[])),key=lambda t:t['name']),
        'spines':[{'table':r['anchor'],'interface':r['interface'],'cost_program':r['operations'],'cost_result':r['result']} for r in joint['recipes']],
        'stages':stages,'patch_components':['rows','parents','fact_parents','watermarks','meta','function_map','cost_rows','candidate_rows','function_rows','choice_rows','mutable_rows','root_state'],
        'patch_storage':'exact u32 offset/value updates; component lengths may change',
        'shared_immutable_components':['markers','roots','rootmap','set_arena'],
        'identity_domain':'one joint invocation; old versions never use the latest union-find',
        'publication':'existing complete rebuild at the joint exit; reacquire interfaces at the next invocation'}
    value=json.loads(json.dumps(value))
    return {'contract':value,'contract_sha256':digest(value)}

def load(path,expected):
    x=json.loads(Path(path).read_text())
    if x.get('schema')!='baked-multi-stage-cell/v1':raise ValueError('unsupported multi-stage Cell schema')
    if x.get('contract_sha256')!=digest(x.get('contract')):raise ValueError('corrupted multi-stage Cell contract')
    if {k:x.get(k) for k in expected}!=expected:raise ValueError('baked Cell rule environment, interfaces or schedule differ')
    if not x.get('evidence',{}).get('accepted_scopes'):raise ValueError('baked Cell has no executed scope evidence')
    return x

def save(path,expected,report,stage_report):
    p=Path(path)
    if p.exists():raise ValueError('baked Cell already exists')
    calls=report['joint_polyspine']['calls']
    if not report['complete_schedule'] or not calls or not all(c['accepted'] for c in calls):
        raise ValueError('Bake needs a complete run with accepted joint scopes')
    if not stage_report['scope_count'] or stage_report['patches']!=sum(c['private_phases'] for c in calls):
        raise ValueError('Bake is missing stage patches')
    result={'schema':'baked-multi-stage-cell/v1',**expected,'evidence':{
        'source_sha256':report['source_sha256'],'program_sha256':report['program_sha256'],
        'accepted_scopes':len(calls),'stages':stage_report['patches'],
        'native_final_checked':bool(report['checks'] and report['checks'][-1]['exact_rows_values_equivalence_visibility']),
        'patch_audit':stage_report['audit'],'source_facts_excluded_from_template_key':True}}
    p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(result,indent=2)+'\n')
    return result

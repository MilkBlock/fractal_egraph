"""Compile native analyzer evidence into executable, guarded multi-stage Cells.

First admitted family: fresh, single-spine extraction contexts. Discovery is
observed; generalization is justified by typed helper/choice contracts and live
singleton guards, never by frequency. The runtime consumes these operations.
"""
import argparse,copy,hashlib,json
from pathlib import Path
from .closure_contract import helper_contract
from .update_recipe import derive_update
from experiments.polyspine_ridge.resumable_program import compile_links
from experiments.device_backend.matrix_comparison import recognize

def digest(x):return hashlib.sha256(json.dumps(x,sort_keys=True,separators=(',',':')).encode()).hexdigest()
def environment(core):
    return {'tables':[t for t in core['tables'] if not t.get('internal') and t['name'] not in core.get('assertion_tables',[])],
            'rulesets':[r for r in core['rulesets'] if r['name'] in ['terms','terms-helpers','terms-helpers-helpers']]}
def source_fingerprint(text):
    h=0xcbf29ce484222325
    for b in text.encode():h=((h^b)*0x100000001b3)&((1<<64)-1)
    return {'algorithm':'fnv1a64','bytes':len(text.encode()),'value':f'{h:016x}'}

def derive(core,rule_name):
    sets={s['name']:s for s in core['rulesets']};tables={t['name']:t for t in core['tables']}
    helpers=sets['terms-helpers']['rules']
    if len(helpers)!=3:raise ValueError('complete helper environment required')
    for i,r in enumerate(helpers):helper_contract(r,i,tables)
    cmp=recognize(sets['terms-helpers-helpers'],tables)
    if (cmp['cost_table'],cmp['choice_table'])!=('TCPair','Smaller'):raise ValueError('unsupported closure interface')
    fn=tables['ExtractedExpr']
    if fn['merge_expr']!={'call':{'table':'Smaller'},'args':[{'var':'old'},{'var':'new'}]}:raise ValueError('changed function merge')
    affine=compile_links(core)
    recipe=next(x for x in affine['recipes'] if x['name']==rule_name)
    seed=next(x for x in sets['terms']['rules'] if x['name']==rule_name)
    anchor=next(a for a in seed['body'] if a['call'].get('table')==recipe['anchor'])
    if any('var' not in a for a in anchor['args']) or len({a['var'] for a in anchor['args']})!=len(anchor['args']):raise ValueError('anchor restrictions require specialization')
    env={a['var']:{'slot':i} for i,a in enumerate(anchor['args'])};params=[{'kind':'anchor','column':i,'sort':a['sort']} for i,a in enumerate(anchor['args'])]
    for port in recipe['inputs']:
        for field in ['term','cost','return']:
            name=port[field]
            if name in env:raise ValueError('cross-port alias constraint')
            env[name]={'slot':len(params)};params.append({'kind':field,'child_column':port['column']})
    # Every non-anchor read is exactly one function/cost input port; no hidden atom.
    role_atoms=[a for a in seed['body'] if a is not anchor]
    if len(role_atoms)!=2*len(recipe['inputs']) or any(a['call'].get('table') not in ['TCPair','ExtractedExpr'] for a in role_atoms):raise ValueError('extra boundary condition')
    n=len(params);stages=[];allops=[]
    def value(a):
        if 'literal' in a:return {'literal':a['literal'],'sort':a.get('sort',{'kind':'i64'})}
        return copy.deepcopy(env[a['var']])
    def emit(op,**kw):
        nonlocal n
        x={'op':op,**kw,'out':n};n+=1;allops.append(x);return {'slot':x['out']}
    candidate=cost=term=None
    for a in seed['head']:
        if a['op']=='copy':env[a['dst']]=value(a['arg']);continue
        if a['op']!='let':raise ValueError('seed effects must construct local rows')
        call=a['call'];args=list(map(value,a['args']))
        if 'table' in call:
            t=tables[call['table']]
            if t['kind']!='constructor':raise ValueError('seed reads or updates a function')
            if t['schema'][-1]['name']==fn['schema'][0]['name']:raise ValueError('seed changes structural carrier')
            if t['schema'][-1]['name']==cmp['sort'] and t['name']!='TCPair':raise ValueError('extra cost constructor')
            dst=emit('construct',table=t['name'],args=args)
            if t['name']=='TCPair':
                if candidate is not None:raise ValueError('multiple seed candidates')
                candidate,term,cost=dst,args[0],args[1]
        else:
            if call.get('primitive') not in ['+','-'] or call['output']!='i64':raise ValueError('unsupported exact primitive')
            dst=emit('integer',operator=call['primitive'],args=args)
        env[a['dst']]=dst
    if candidate is None or allops[-1].get('table')!='PotentialExtractedExpr':raise ValueError('missing seed output')
    root={'slot':len(anchor['args'])-1}
    if allops[-1]['args']!=[root,candidate]:raise ValueError('candidate has a different owner')
    stages.append({'ruleset':'terms','operations':copy.deepcopy(allops)})
    seed_end=len(allops)
    init_head=helpers[0]['head'];zero=[a for a in init_head if a['op']=='let' and 'table' in a['call']]
    initial_term=emit('construct',table=zero[0]['call']['table'],args=[])
    sentinel=emit('construct',table='TCPair',args=[initial_term,{'literal':affine['initializer_cost'],'sort':{'kind':'i64'}}])
    stages.append({'ruleset':'terms-helpers','operations':copy.deepcopy(allops[seed_end:]),'pending_function':sentinel})
    merge=emit('construct',table='Smaller',args=[sentinel,candidate])
    stages.append({'ruleset':'terms-helpers','operations':[copy.deepcopy(allops[-1])],'pending_function':merge})
    union=emit('union',args=[merge,candidate])
    publish=emit('publish',table='ExtractedExpr',args=[root,merge])
    stages.append({'ruleset':'terms-helpers-helpers','operations':copy.deepcopy(allops[-2:])})
    return {'rule':rule_name,'anchor':recipe['anchor'],'parameters':params,'slots':n,'stages':stages,
            'outputs':{'candidate':candidate,'term':term,'cost':cost,'sentinel':sentinel,'choice':merge,'root':root},
            'guards':{'fresh_function':True,'fresh_candidates':True,'one_spine':True,'singleton_cost_inputs':True,
                      'cost_interval':[0,affine['initializer_cost']],'exclusive_upper':True,'no_new_cost_alias':True},
            'scope':'guarded fresh extraction Cell; existing/multiple-candidate contexts use the ordinary program'}

def compile_library(core,analysis,source,updates=False,updates_only=False):
    cell=analysis.get('saturated_rule_composition',{}).get('cell_egraph',analysis)
    evidence=cell.get('executable_program_evidence',{})
    if evidence.get('schema')!='analyzer-cell-program-evidence/v1':raise ValueError('native analyzer evidence is missing')
    if evidence['source_fingerprint']!=source_fingerprint(source):raise ValueError('analysis source differs')
    observed={(r['ruleset'],r['ordinal']):r for r in evidence['rules']}
    if not all(any(r['ruleset']==s for r in observed.values()) for s in ['terms','terms-helpers','terms-helpers-helpers']):raise ValueError('multi-stage native history is incomplete')
    templates=[];rejected=[]
    for ordinal,rule in enumerate(next(s['rules'] for s in core['rulesets'] if s['name']=='terms')):
        witness=observed.get(('terms',ordinal))
        if witness is None:rejected.append({'rule':rule['name'],'reason':'no native analyzer application'});continue
        try:
            t=derive(core,rule['name'])
            operators=[a['call'].get('table',a['call'].get('primitive')) for a in rule['body']]
            if sorted(operators)!=sorted(witness['body_operators']):raise ValueError('native rule ordinal/body mismatch')
        except (ValueError,KeyError,StopIteration) as error:rejected.append({'rule':rule['name'],'reason':str(error)});continue
        t['evidence']={'analyzer_rule':witness['id'],'applications':witness['applications'],'example':witness['example']}
        templates.append(t)
    update=derive_update(core) if updates or updates_only else None
    update_evidence=observed.get(('terms-helpers',1),{})
    if update and not update_evidence.get('existing_value_requests',0):raise ValueError('no native evidence of an existing function value update')
    if updates_only:templates=[]
    if not templates and update is None:raise ValueError('no executable Cell admitted')
    payload={'schema':'executable-multistage-cell/v1','environment':environment(core),'templates':templates,
             'evidence':{'source_sha256':hashlib.sha256(source.encode()).hexdigest(),'cell_eclasses':cell.get('eclasses',0),'producer_edges':evidence['producer_edges'],'scope':evidence['scope']},
             'rejected':rejected,'proof':'native observations select spines; typed total-min-choice contract plus fresh/singleton runtime guards certify these finite recipes'}
    if update:
        payload['schema']='executable-multistage-cell/v2';payload['update_program']=update;payload['update_evidence']=update_evidence
        payload['proof']='observed native transitions; typed complete helper/min-choice proof; correlated bag inputs and exact stage publication'
    return {**payload,'sha256':digest(payload)}

def load(path,core):
    x=json.loads(Path(path).read_text());payload={k:v for k,v in x.items() if k!='sha256'}
    if x.get('schema') not in ['executable-multistage-cell/v1','executable-multistage-cell/v2'] or x.get('sha256')!=digest(payload):raise ValueError('invalid executable Cell artifact')
    if x['environment']!=environment(core):raise ValueError('Cell program rule environment changed')
    if x['schema'].endswith('/v2'):
        if x.get('update_program')!=derive_update(core):raise ValueError('Cell update program failed typed proof validation')
        if not x.get('update_evidence',{}).get('existing_value_requests',0):raise ValueError('Cell update lacks old-value evidence')
    for t in x['templates']:
        if {k:v for k,v in t.items() if k!='evidence'}!=derive(core,t['rule']):raise ValueError('Cell program failed typed proof validation')
        if t['evidence']['applications']<=0:raise ValueError('Cell lacks native evidence')
    return x

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--analysis',type=Path,required=True);p.add_argument('--core',type=Path,required=True);p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--updates',action='store_true');p.add_argument('--updates-only',action='store_true');a=p.parse_args()
    if a.output.exists():raise ValueError('output already exists')
    program=compile_library(json.loads(a.core.read_text()),json.loads(a.analysis.read_text()),a.source.read_text(),updates=a.updates,updates_only=a.updates_only)
    a.output.write_text(json.dumps(program,indent=2)+'\n');print('exported',len(program['templates']),'fresh Cell recipes; bag update program:',bool(program.get('update_program')))

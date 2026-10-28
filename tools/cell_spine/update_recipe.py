"""Typed bag-valued continuation for existing extraction owners.

A candidate is a correlated row witness, not a minimum-cost representative.
The helper operations are exported from the native AST; ordering and witness
multiplicity are part of the contract because custom merge is not idempotent.
"""
from .closure_contract import helper_contract
from experiments.device_backend.matrix_comparison import recognize

def derive_update(core):
    sets={s['name']:s for s in core['rulesets']};tables={t['name']:t for t in core['tables']}
    rules=sets['terms-helpers']['rules']
    if len(rules)!=3:raise ValueError('complete helper program required')
    fn=tables['ExtractedExpr']
    if fn['merge_expr']!={'call':{'table':'Smaller'},'args':[{'var':'old'},{'var':'new'}]}:raise ValueError('unsupported update merge')
    comparison=recognize(sets['terms-helpers-helpers'],tables)
    if (comparison['cost_table'],comparison['choice_table'])!=('TCPair','Smaller'):raise ValueError('unsupported comparison')
    stages=[]
    for rank,r in enumerate(rules):
        roles=helper_contract(r,rank,tables);params=[];env={}
        for atom,role in zip([a for a in r['body'] if 'table' in a['call']],roles):
            for col,a in enumerate(atom['args']):
                if a['var'] not in env:
                    env[a['var']]={'slot':len(params)};params.append({'role':role,'column':col,'sort':a['sort']})
        def value(a):return {'literal':a['literal'],'sort':a['sort']} if 'literal' in a else env[a['var']]
        ops=[];n=len(params)
        for a in r['head']:
            if a['op']=='copy':env[a['dst']]=value(a['arg']);continue
            if a['op']=='let':
                op={'op':'construct','table':a['call']['table'],'args':[value(x) for x in a['args']],'out':n}
                env[a['dst']]={'slot':n};n+=1
            elif a['op']=='set':op={'op':'set','table':a['call']['table'],'args':[value(x) for x in a['args']],'value':value(a['value'])}
            elif a['op']=='panic':op={'op':'panic'}
            else:raise ValueError('unsupported helper instruction')
            ops.append(op)
        stages.append({'rule':r['name'],'rank':rank,'roles':roles,'parameters':params,'slots':n,
                       'seminaive':r['seminaive'],'guard':['true','candidate_cost < old_cost','candidate_cost < 0'][rank],
                       'operations':ops})
    return {'schema':'bag-extraction-update/v1','ruleset':'terms-helpers','stages':stages,'comparison':comparison,
            'ports':['PotentialExtractedExpr(owner,candidate)','ExtractedExpr(owner,old)','TCPair witnesses by canonical class'],
            'dependencies':['candidate row generation','function row generation','cost-class membership and row generation'],
            'ordering':'native rule rank, delta pivot, seed and correlated witness order',
            'multiplicity':'preserve every enabled set request; never collapse to a winner',
            'scope':'existing or fresh owners, any finite candidate/spine multiplicity; exact guards and native stage boundaries'}

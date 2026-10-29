"""Boundary recipes over the exact marker/port ABI, without fresh-Cell guards."""
from .resumable import compile_links

def derive_spine(core,rule_name,derive_fresh):
    # The fresh proof already validates the complete seed body/effects. Only
    # its seed stage is reused; fresh/singleton assumptions are NOT imported.
    full=derive_fresh(core,rule_name)
    r=next(r for s in core['rulesets'] if s['name']=='terms' for r in s['rules'] if r['name']==rule_name)
    linked=next(x for x in compile_links(core)['recipes'] if x['name']==rule_name)
    roles=[]
    for a in r['body']:
        name=a['call'].get('table')
        if name==linked['anchor']:roles.append(0)
        elif name=='ExtractedExpr':roles.append(1+next(i for i,p in enumerate(linked['inputs']) if p['key']==a['args'][0]['var']))
        elif name=='TCPair':roles.append(4+next(i for i,p in enumerate(linked['inputs']) if p['return']==a['args'][-1]['var']))
        else:raise ValueError('unsupported boundary guard')
    ops=full['stages'][0]['operations'];slots=max([len(full['parameters'])]+[x['out']+1 for x in ops])
    return {'rule':rule_name,'anchor':full['anchor'],'parameters':full['parameters'],'roles':roles,
            'input_columns':[p['column'] for p in linked['inputs']], 'slots':slots,'operations':ops,
            'numeric_program':{'operations':[list(op) for op in linked['operations']],'coefficients':list(linked['coefficients']),'result':list(linked['result'])},
            'scope':'all correlated live child witnesses; exact phase frontier; no fresh-root or singleton restriction'}

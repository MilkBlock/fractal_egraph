"""Shared typed marker/affine compilation; usable without NumPy on the runtime path."""
from .affine import compile_program,HI

def compile_links(core):
    try:p=compile_program(core)
    except (KeyError,StopIteration) as error:raise ValueError('unsupported terms interface') from error
    if p['max_registers']>16 or not all(r['matrix_range_certificate'] for r in p['recipes']):raise ValueError('GPU arithmetic representation not certified')
    p['heads']=[r['head'] for rs in core['rulesets'] if rs['name']=='terms' for r in rs['rules']]
    defaults=[]
    for rs in core['rulesets']:
        if rs['name']=='terms-helpers':
            for r in rs['rules']:
                for a in r['head']:
                    if a.get('call',{}).get('table')=='TCPair' and 'literal' in a['args'][1]:defaults.append(int(a['args'][1]['literal']))
    if len(set(defaults))!=1:raise ValueError('requires one observed native initializer cost')
    if not 0<=defaults[0]<=HI:raise ValueError('initializer outside nonnegative i64 domain')
    p['initializer_cost']=defaults[0]
    # This projection uses monotonicity of these actual cost expressions. Do
    # not silently apply min-of-inputs to a nonmonotone or guarded recipe.
    for r in p['recipes']:
        if any(c<0 for c in r['coefficients']) or any(op!=0 or (ak and a<0) or (bk and b<0) for op,ak,a,bk,b in r['operations']):raise ValueError('nonmonotone cost program')
        names=[x for port in r['inputs'] for x in [port['term'],port['cost'],port['return']]]
        if len(names)!=len(set(names)):raise ValueError('cross-port alias constraints require a richer projection')
    p['copy_recipe']=len(p['recipes']);p['min_recipe']=len(p['recipes'])+1
    p['schema']='resumable-terms-context/v1'
    return p

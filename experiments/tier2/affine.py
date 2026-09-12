"""Exact translation inference, with source-AST coefficient checks.
No recurrence-specific constants are embedded in the learner.
"""
import json,math
from pathlib import Path
OUT=Path(__file__).parent

def infer(samples):
    if len(samples)<2:return None
    deltas={(y[0]-x[0],y[1]-x[1]) for x,y in samples}
    return next(iter(deltas)) if len(deltas)==1 else None

def fit_trigger(train,min_support=3):
    # Only the training prefix chooses the cut. Heldout observations never move it.
    for cut in range(len(train)-min_support+1):
        delta=infer(train[cut:])
        if delta is not None:return cut,delta
    return None,None

def invariant(delta):
    dm,da=delta
    if not(dm or da):return None
    x,y=da,-dm;g=math.gcd(abs(x),abs(y));x//=g;y//=g
    if x<0 or x==0 and y<0:x=-x;y=-y
    return x,y

def affine(e,var):
    if e.get('var')==var:return (1,0)
    if 'literal' in e:return (0,int(e['literal']))
    op=e.get('op');args=[affine(x,var) for x in e.get('args',[])]
    if len(args)!=2:raise ValueError('unsupported affine expression')
    (a,b),(c,d)=args
    if op=='+':return a+c,b+d
    if op=='-':return a-c,b-d
    if op=='*' and a*c==0:return a*d+b*c,b*d
    raise ValueError('non-affine expression')

def source_certificate(ast,delta,inv):
    """Check the chosen F/Add/Const language adapter, not arbitrary egglog effects."""
    checked=[]
    for name,r in ast.items():
        try:
            root,lhs=r['body'][0]['eq']
            if lhs.get('op')!='F':continue
            var=lhs['args'][0]['var']
            a,rhs=r['head'][0]['union']
            if a!=root or rhs.get('op')!='Add':raise ValueError('not an additive unfolding')
            recursive,weight=rhs['args']
            if recursive.get('op')!='F' or weight.get('op')!='Const':raise ValueError('unsupported recursion shape')
            cm,dm=affine(recursive['args'][0],var);cw,da=affine(weight['args'][0],var)
            if cm!=1 or cw!=0:raise ValueError('not a translation')
            if (dm,da)!=tuple(delta):return {'status':'source_counterexample','rule':name,'delta':[dm,da]}
            if inv[0]*dm+inv[1]*da!=0:raise ValueError('invariant fails')
            checked.append({'rule':name,'delta':[dm,da],'guards':r['body'][1:]})
        except (KeyError,ValueError,TypeError) as e:return {'status':'unsupported','reason':str(e)}
    return {'status':'translation_identity_checked' if checked else 'unsupported','checked_rules':checked,
        'meaning':'Exact coefficient identity for the additive-recursion adapter, over mathematical integers and associative addition. Does not prove equality of full egraph effects or machine-i64 overflow safety.'}

def path_samples(edges,start):
    graph={}
    for n,m,w in edges:
        if n in graph and graph[n]!=(m,w):raise ValueError('ambiguous recurrence edge')
        graph[n]=(m,w)
    m=start;a=0;seen=set();samples=[]
    while m in graph:
        if m in seen:raise ValueError('cycle in recurrence observations')
        seen.add(m);m2,w=graph[m];samples.append(((m,a),(m2,a+w)));m,a=m2,a+w
    return samples

def main():
    observations=json.loads((OUT/'observations.json').read_text())['samples']
    report=[];text=['(include "experiments/tier2/ir.egg")']
    for name in ['unit','triple','stride','changing','warmup']:
        data=json.loads((OUT/'fixtures'/f'{name}.json').read_text())
        samples=[(tuple(s['before']),tuple(s['after'])) for s in observations if s['series']==name]
        assert samples==path_samples(data['edges'],24), 'native tier-1 interface differs from checked tier-0 path'
        train=samples[:5];heldout=samples[5:];cut,delta=fit_trigger(train);inv=invariant(delta) if delta else None
        passed=sum((b[0]-a[0],b[1]-a[1])==delta for a,b in heldout)
        cert=source_certificate(json.loads((OUT/'fixtures'/f'{name}.ast.json').read_text()),delta,inv) if inv else {'status':'unsupported'}
        if delta:
            text.append(f'(Proposed "{name}" (Translate {delta[0]} {delta[1]}))')
            if inv:text.append(f'(CheckLinear (Translate {delta[0]} {delta[1]}) {inv[0]} {inv[1]})')
            text.append(f'(Power (Translate {delta[0]} {delta[1]}) {len(samples)})')
            for a,b in samples[cut:]:text.append(f'(Sample "{name}" {a[0]} {a[1]} {b[0]} {b[1]})')
        report.append({'name':name,'train':len(train),'trigger_depth':cut,'trigger_state':samples[cut][0] if cut is not None else None,'heldout':len(heldout),'heldout_pass':passed,'delta':delta,'invariant_coefficients':inv,'observed_invariant_constant':sum(x*y for x,y in zip(inv,samples[cut][0])) if inv else None,
            'certificate':cert,'samples':samples,'family':f'f(n) = f(n + k*({delta[0]})) + k*({delta[1]})' if delta else None,
            'domain':'k >= 0; every source guard on the k-step path must hold; native i64 calculations must not overflow',
            'effect':'Endpoint equality only. Intermediate F/Add nodes and unions are not removed or identified with a single jump.'})
    text.append('(run-schedule (saturate (run tier2)))')
    for r in report:
        if r['delta']:
            x,y=r['delta'];k=len(r['samples'])
            text.append(f'(check (= (Power (Translate {x} {y}) {k}) (Translate {x*k} {y*k})))')
            if r['invariant_coefficients']:
                a,b=r['invariant_coefficients'];text.append(f'(check (PreservesLinear (Translate {x} {y}) {a} {b}))')
    (OUT/'affine.egg').write_text('\n'.join(text)+'\n');(OUT/'affine.json').write_text(json.dumps(report,indent=2)+'\n')
    print([(r['name'],r['delta'],r['heldout_pass'],r['heldout'],r['certificate']['status']) for r in report])
if __name__=='__main__':main()

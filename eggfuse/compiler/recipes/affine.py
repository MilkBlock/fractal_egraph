"""Typed affine extraction and exact i64 evaluation; no training dependency."""
LO=-(1<<63);HI=(1<<63)-1


def compile_program(core):
    rules=next(r['rules'] for r in core['rulesets'] if r['name']=='terms');out=[]
    for r in rules:
        anchors=[a for a in r['body'] if a['call'].get('table') not in ['TCPair','ExtractedExpr']]
        if len(anchors)!=1 or 'table' not in anchors[0]['call']:raise ValueError('unsupported structural anchor')
        anchor=anchors[0];cols={a['var']:i for i,a in enumerate(anchor['args'])}
        extracts={a['args'][-1]['var']:a['args'][0]['var'] for a in r['body'] if a['call'].get('table')=='ExtractedExpr'}
        inputs=[]
        for a in r['body']:
            if a['call'].get('table')=='TCPair':
                term,cost,ret=a['args'];key=extracts[ret['var']]
                inputs.append({'column':cols[key],'key':key,'term':term['var'],'cost':cost['var'],'return':ret['var']})
        if len(inputs)>3:raise ValueError('packet supports up to three independent cost inputs')
        refs={p['cost']:(0,i) for i,p in enumerate(inputs)}
        affine={p['cost']:[int(j==i) for j in range(4)] for i,p in enumerate(inputs)}
        ops=[];final=None
        def ref(a):return (1,int(a['literal'])) if 'literal' in a else refs[a['var']]
        def linear(a):return [0,0,0,int(a['literal'])] if 'literal' in a else affine[a['var']]
        for a in r['head']:
            if a['op']=='copy':
                if a['arg']['var'] in refs:refs[a['dst']]=refs[a['arg']['var']];affine[a['dst']]=affine[a['arg']['var']]
            elif a['op']=='let' and 'primitive' in a['call']:
                op=a['call']['primitive']
                if op not in ['+','-'] or a['call']['output']!='i64':raise ValueError('non-affine primitive')
                left,right=map(ref,a['args']);ops.append([0 if op=='+' else 1,*left,*right])
                refs[a['dst']]=(0,3+len(ops)-1)
                aa,bb=map(linear,a['args']);affine[a['dst']]=[x+y if op=='+' else x-y for x,y in zip(aa,bb)]
            elif a['op']=='let' and a['call'].get('table')=='TCPair':
                final=ref(a['args'][1]);coeff=list(linear(a['args'][1]))
        if final is None:raise ValueError('missing TCPair result')
        # For admitted formulas, |each input|<=2048 makes the two final
        # affine columns exact in FP32. The original i64 instruction DAG is
        # checked separately so reassociation cannot hide intermediate overflow.
        bound=sum(abs(c)*2048 for c in coeff[:3])+abs(coeff[3])
        safe_coeff=all(abs(c)<=2048 for c in coeff) and bound+2048<(1<<24)
        out.append({'name':r['name'],'anchor':anchor['call']['table'],'args':anchor['args'],'inputs':inputs,
                    'coefficients':coeff,'operations':ops,'result':final,'matrix_range_certificate':safe_coeff})
    return {'schema':'native-affine-terms/v1','recipes':out,'max_registers':3+max(len(r['operations']) for r in out)}


def exact(recipe,costs,cold,complete=True,has_old=True):
    if not complete:return 0,0,0
    regs=list(map(int,costs));valid=True
    for op,ak,a,bk,b in recipe['operations']:
        av=a if ak else regs[a];bv=b if bk else regs[b];v=av+bv if op==0 else av-bv
        if not LO<=v<=HI:valid=False;break
        regs.append(v)
    if not valid:return 0,0,0
    kind,v=recipe['result'];new=v if kind else regs[v];delta=int(cold)-new
    flags=1 | (2 if has_old and new<cold else 0) | (4 if not has_old else 0) | (8 if new<0 else 0)
    if has_old and LO<=delta<=HI:flags|=16
    else:delta=0
    if recipe['matrix_range_certificate'] and all(abs(int(c))<=2048 for c in [*costs,cold]):flags|=32
    return new,delta,flags

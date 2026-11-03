"""Exact RHS slices: parallel immutable construction, ordered residual effects.

Packet references are local names, not provisional global e-class IDs. All
fallible binding-only arithmetic is checked before any packet is published.
Callback constructors must be disjoint from moved constructor tables; no
mutable function read, subsume, container operation or arbitrary callback is
reordered. Unproved programs keep the original executor.
"""
MINMAX={'(min old new)','(min new old)','(max old new)','(max new old)'}


def callback_tables(table,tables):
    if table.get('merge')=='new':return set(),0
    if table.get('merge')=='old':return (set(),0) if table['schema'][-1]['name']=='Unit' else None
    if table.get('merge') in MINMAX and table['schema'][-1]['kind']=='i64':return set(),0
    names={t['name']:t for t in tables}
    def walk(e):
        if 'var' in e or 'literal' in e:return set(),0
        call=e.get('call',{});t=names.get(call.get('table'))
        if t is None or t['kind']!='constructor' or t.get('merge') is not None or any(x['kind']=='set' for x in t['schema']):raise ValueError('non-constructor callback')
        used={t['name']};n=1
        for x in e.get('args',[]):
            u,k=walk(x);used|=u;n+=k
        return used,n
    try:return walk(table['merge_expr'])
    except (KeyError,ValueError,TypeError):return None


def classify(ins,width,tables):
    if not width:return None,'grounded initializer remains compact'
    deps={};args={};constructs=[];depths=[];callbacks=set();tail_rows=tail_ids=tail_unions=0;tail=False
    for op,dst,sub,k,value in ins:
        dep=deps.get(value,0) if k==1 else 0
        if op==0:args[dst]=dep
        elif op==1:deps[dst]=dep
        elif op==3:
            t=tables[sub]
            if any(x['kind']=='set' for x in t['schema']):return None,'container key'
            if t['kind']=='constructor' and t.get('merge') is None:
                depth=max((args.get(i,0) for i in range(len(t['schema'])-1)),default=0)
                constructs.append(sub);depths.append(depth);deps[dst]=depth+1
            else:return None,'mutable RHS function read'
        elif op==4:
            t=tables[sub]
            if t['kind']=='constructor' or any(x['kind']=='set' for x in t['schema']):return None,'constructor set or container update'
            proof=callback_tables(t,tables)
            if proof is None:return None,'fallible or effectful merge'
            used,n=proof;callbacks|=used;tail_rows+=n+1;tail_ids+=n;tail_unions+=n;tail=True
        elif op==2:tail=True;tail_unions+=1
        elif op in (6,7):
            count=(value if sub==4 else 1 if sub==0 else 2) if op==7 else 2
            if any(args.get(i,0) for i in range(count)):return None,'arithmetic consumes a constructor identity'
            deps[dst]=0
        else:return None,'masked, container or unsupported effect'
    if not constructs:return None,'no construction to batch'
    if callbacks&{tables[t]['name'] for t in constructs}:return None,'callback observes moved table'
    return dict(writes=len(constructs),depth=max(depths)+1,depths=depths,tail=tail,
                tail_rows=tail_rows,tail_ids=tail_ids,tail_unions=tail_unions),None


def emit(ins,width,variables,tables,info,name,cell=False,steps=0):
    """One SSA emitter, two modes: stage symbolic requests or replay exact values.

    Tail mode never calls constructors again: it reads their resolved packet.
    Cell mode only admits full pure cones; ordinary tails retain binding order.
    """
    cols=max(len(t['schema']) for t in tables)
    lines=[f'long v{i}='+ (f'input[{i}]' if cell else f'matches[binding*MSTRIDE+1+{i}]')+';' if i<width else f'long v{i}=0;' for i in range(variables)]
    lines += [f'long a{i}=0;' for i in range(cols)]
    effect=0
    def value(k,x):return f'matches[binding*MSTRIDE+1+{x}]' if k==2 else f'v{x}' if k else ('(-9223372036854775807L-1L)' if x==-(1<<63) else str(x)+'L')
    for op,dst,sub,k,x in ins:
        v=value(k,x)
        if op==0:lines.append(f'a{dst}={v};')
        elif op==1:lines.append(f'v{dst}={v};')
        elif op==3:
            t=tables[sub];depth=info['depths'][effect]
            lines.append(f'if(!tail){{device long*q=packets+(base+{effect})*RP_STRIDE;q[0]={sub};q[1]={depth};q[2]=-1;')
            lines.extend(f'q[{3+i}]=a{i};' for i in range(len(t['schema'])-1))
            lines.append('}')
            if t['kind']=='constructor':lines.append(f'v{dst}=tail?rp_result(base+{effect},packets,w,r,p):(-1L-long(base+{effect}));')
            else:lines.append(f'v{dst}=0;')
            effect+=1
        elif op==6:lines.append(f'if(!integer_valid({sub},a0,a1))return false;v{dst}=integer_eval({sub},a0,a1);')
        elif op==7:
            e={0:'!a0',1:'a0&&a1',2:'a0!=a1',3:'!a0||a1'}.get(sub)
            if sub==4:e='|'.join(f'a{i}' for i in range(x)) or '0L'
            lines.append(f'v{dst}=({e});')
        elif op==2:lines.append('if(tail){unite(p,a0,a1,m);if(m.error)return false;}')
        elif op==4:
            args=','.join(f'a{i}' for i in range(len(tables[sub]['schema'])-1)) or '0'
            lines.append(f'if(tail){{long key[COLS]={{{args}}};write_row(r,p,fp,m,{sub},key,a{len(tables[sub]["schema"])-1},false);if(m.error)return false;}}')
    if cell:
        lines.append('if(tail){')
        for i in range(variables):lines.append(f'input[{i}]=v{i};input[BC_WIDTH+{i}]=1;')
        for i in range(steps):lines.append(f'input[2*BC_WIDTH+{i}]=1;')
        lines.append('}')
    lines.append('return true;')
    signature='uint binding,device long*matches,device long*input,device long*packets,device uint*w,device long*r,device uint*p,device uint*fp,device Meta&m,uint base,bool tail'
    return f'__attribute__((noinline)) bool {name}({signature}){{'+''.join(lines)+'}'


def lower(program,core,regions,batch_layout):
    layout=program['action_layout'];tables=program['tables'];code=program['action_code'];parts=[];rules=[];rejected=[];cases=[]
    for rid,(lo,hi) in enumerate(zip(layout['starts'],layout['starts'][1:])):
        ins=[code[i:i+5] for i in range(lo,hi,5)]
        info,reason=classify(ins,layout['query_widths'][rid],tables)
        rules.append(info)
        if info is None:rejected.append(dict(rule=rid,reason=reason));continue
        parts.append(emit(ins,layout['query_widths'][rid],layout['variable_counts'][rid],tables,info,f'rp_rule_{rid}'))
        cases.append(f'case {rid}:return rp_rule_{rid}(binding,matches,input,packets,w,r,p,fp,m,base,tail);')
    cells,cell_parts,cell_cases=cell_cones(core,regions,batch_layout,tables,program['strings'])
    parts+=cell_parts
    signature='int plan,uint binding,device long*matches,device long*input,device long*packets,device uint*w,device long*r,device uint*p,device uint*fp,device Meta&m,uint base,bool tail'
    parts.append('bool rp_recipe('+signature+'){if(plan>=0){switch(plan){'+''.join(cell_cases)+'default:return false;}}switch(uint(matches[binding*MSTRIDE])){'+''.join(cases)+'default:return false;}}')
    for label,key in [('WRITES','writes'),('DEPTH','depth'),('TAIL_ROWS','tail_rows'),('TAIL_IDS','tail_ids'),('TAIL_UNIONS','tail_unions'),('TAIL','tail'),('STEPS','steps'),('EFFECTS','effects')]:
        parts.insert(0,'constant uint RP_'+label+'[]={'+(','.join(str(int(x.get(key,0))) if x else '0' for x in rules) or '0')+'};')
        parts.insert(0,'uint rp_cell_'+key+'(int plan){switch(plan){'+''.join(f'case {i}:return {int(x.get(key,0))};' for i,x in cells.items())+'default:return 0;}}')
    parts.insert(0,'constant uint RP_RULE[]={'+(','.join('1' if x else '0' for x in rules) or '0')+'};')
    return '\n'.join(parts),dict(schema='eggfuse-rhs-packets/v1',rules=rules,rejected=rejected,cells=cells,
        contract='local constructor references; frozen alias epoch; parallel hashcons; exact ordered residual; pure witnessed multi-stage cones')


def cell_cones(core,regions,batch_layout,tables,strings):
    from eggfuse.ir.recipe import build,digest
    from .compiler import integer_op,boolean_expr
    from .cell_waves import eligible
    recipe=build({k:v for k,v in core.items() if k!='cell_input_constants'},regions['scope'])
    tids={t['name']:i for i,t in enumerate(tables)};sids={s:i for i,s in enumerate(strings)}
    unique=[];seen=set()
    for t in regions['batch_program']['templates']:
        key=digest({k:v for k,v in t.items() if k!='scope'})
        if key not in seen:seen.add(key);unique.append(t)
    result={};parts=[];cases=[]
    for ti,t in enumerate(unique):
        if len(t['steps'])<2 or not eligible(t,recipe):continue
        ins=[];symbols={i:('slot',i) for i in range(t['input_width'])};facts=set();failed=False
        def mapped(step,x):return step['mapping'][x['slot']] if 'slot' in x else x
        def symbol(x):
            if 'slot' in x:return symbols.get(x['slot'],('undefined',x['slot']))
            return ('literal',x['sort']['name'],x['literal'])
        def operand(x):
            if 'slot' in x:return 1,x['slot']
            return 0,sids[x['literal']] if x['sort']['kind']=='String' else int(x['literal'])
        def load(args):
            for j,x in enumerate(args):k,v=operand(x);ins.append([0,j,0,k,v])
        def fact(call,args):return (call['table'],tuple(map(symbol,args)))
        for si,step in enumerate(t['steps']):
            r=recipe['rules'][step['recipe_rule']]
            # A virtual fact proves an atom only when its complete correlated
            # key AND return wiring match. No union, external read or guess.
            for a in r['query']:
                args=[mapped(step,x) for x in a['args']]
                if 'table' not in a['call']:
                    if si:failed=True
                    continue
                f=fact(a['call'],args)
                if si and f not in facts:failed=True
                if not si:facts.add(f)
            if failed:break
            for e in r['effects']:
                kind=e['kind'];args=[mapped(step,x) for x in e.get('args',[])];load(args)
                if kind=='copy':
                    x=mapped(step,e['arg']);dst=mapped(step,{'slot':e['dst']})['slot'];k,v=operand(x);ins.append([1,dst,0,k,v]);symbols[dst]=symbol(x)
                elif kind=='construct':
                    dst=mapped(step,{'slot':e['dst']})['slot'];tid=tids[e['call']['table']];ins.append([3,dst,tid,0,0])
                    key=tuple(map(symbol,args));known=next((f[1][-1] for f in facts if f[0]==e['call']['table'] and f[1][:-1]==key),None)
                    symbols[dst]=known or ('node',len(ins))
                    facts.add((e['call']['table'],key+(symbols[dst],)))
                elif kind=='compute':
                    dst=mapped(step,{'slot':e['dst']})['slot'];op=integer_op(e['call'])
                    if op is not None:ins.append([6,dst,op,0,0])
                    elif boolean_expr(e['call'],['x']*len(args)) is not None:ins.append([7,dst,{'not':0,'and':1,'xor':2,'=>':3,'or':4}[e['call']['primitive']],0,len(args)])
                    else:failed=True;break
                    symbols[dst]=('compute',e['call']['primitive'],tuple(map(symbol,args)))
                else:failed=True;break
            if failed:break
        if failed:continue
        info,reason=classify(ins,t['input_width'],tables)
        if not info or info['tail']:continue
        info.update(steps=len(t['steps']),effects=sum(sum(e['kind']!='copy' for e in recipe['rules'][s['recipe_rule']]['effects']) for s in t['steps']))
        result[ti]=info
        parts.append(emit(ins,t['input_width'],len(t['slots']),tables,info,f'rp_cell_{ti}',True,steps=len(t['steps'])))
        cases.append(f'case {ti}:return rp_cell_{ti}(binding,matches,input,packets,w,r,p,fp,m,base,tail);')
    return result,parts,cases

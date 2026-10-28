"""Mechanical lowering of the validated bag update artifact."""
def lower(artifact,core,strings):
    update=(artifact or {}).get('update_program');tables={t['name']:i for i,t in enumerate(core['tables'])}
    stages=update['stages'] if update else []
    width=max([s['slots'] for s in stages]+[1]);bodies=[];effects=[];roles=[]
    def value(x):
        if 'slot' in x:return f'v[{x["slot"]}]'
        n=x['literal'];n=strings[n] if isinstance(n,str) else int(n)
        return '(-9223372036854775807L-1L)' if n==-(1<<63) else str(n)+'L'
    for rank,s in enumerate(stages):
        bodies.append(f'case {rank}:{{'+''.join(f'v[{j}]=rows[selected[{p["role"]}]*STRIDE+4+{p["column"]}];' for j,p in enumerate(s['parameters']))+'break;}')
        roles.append(f'case {rank}:{{const uint role[]={{'+','.join(map(str,s['roles']))+'};return role[atom];}')
        code=[]
        for op in s['operations']:
            if op['op']=='construct':code.append('{long args[COLS]={'+','.join(map(value,op['args']))+'};'+f'v[{op["out"]}]=ensure_constructor(r,p,fp,m,{tables[op["table"]]},args);'+'}')
            elif op['op']=='set':code.append('{long args[COLS]={'+','.join(map(value,op['args']))+'};'+f'write_row(r,p,fp,m,{tables[op["table"]]},args,{value(op["value"])},false);'+'}')
            elif op['op']=='panic':code.append('m.error=8;return;')
            else:raise ValueError('unknown exported update operation')
            code.append('if(m.error)return;')
        effects.append(f'case {rank}:{{'+''.join(code)+'break;}')
    code=f'#define UP_WIDTH {width}\n'
    code+='uint up_role(uint rank,uint atom){switch(rank){'+''.join(roles)+'default:return 0;}}\n'
    code+='template<typename R> void up_parameters(uint rank,thread const uint*selected,R rows,thread long*v){switch(rank){'+''.join(bodies)+'default:break;}}\n'
    code+='void up_execute(uint rank,thread long*v,device long*r,device uint*p,device uint*fp,device Meta&m){switch(rank){'+''.join(effects)+'default:m.error=29;}}\n'
    return code,update

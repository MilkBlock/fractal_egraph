"""Mechanical lowering of loaded boundary recipe slots and constructor effects."""
def lower(artifact,core,strings,joint):
    program=(artifact or {}).get('spine_program');recipes=program['recipes'] if program else []
    native=(joint.get('program') or {}).get('recipes',[])
    native_ids={r['name']:i for i,r in enumerate(native)};tables={t['name']:i for i,t in enumerate(core['tables'])}
    width=max([r['slots'] for r in recipes]+[1]);fields=[];roles=[];effects=[];params=[]
    def value(x):
        if 'slot' in x:return f'v[{x["slot"]}]'
        n=x['literal'];n=strings[n] if isinstance(n,str) else int(n)
        return '(-9223372036854775807L-1L)' if n==-(1<<63) else str(n)+'L'
    for index,r in enumerate(recipes):
        body=[]
        for slot,p in enumerate(r['parameters']):
            if p['kind']=='anchor':role=0;col=p['column']
            else:role=4+r['input_columns'].index(p['child_column']);col={'term':0,'cost':1,'return':2}[p['kind']]
            body.append(f'v[{slot}]=rows[selected[{role}]*STRIDE+4+{col}];')
        fields.append(f'case {index}:{{'+''.join(body)+'break;}')
        roles.append(f'case {index}:{{const uint roles[]={{'+','.join(map(str,r['roles']))+'};return roles[atom];}')
        params.append(f'case {index}:return {len(r["parameters"])};')
        code=[];numeric=0
        for op in r['operations']:
            if op['op']=='integer':code.append(f'v[{op["out"]}]=numeric[{numeric}];');numeric+=1
            elif op['op']=='construct':code.append('{long args[COLS]={'+','.join(map(value,op['args']))+'};'+f'v[{op["out"]}]=write_row(r,p,fp,m,{tables[op["table"]]},args,0,true);'+'}')
            else:raise ValueError('unknown boundary operation')
            code.append('if(m.error)return;')
        if numeric!=len(r['numeric_program']['operations']):raise ValueError('boundary numeric slot mismatch')
        effects.append(f'case {index}:{{'+''.join(code)+'break;}')
    code='int sd_index(uint recipe){switch(recipe){'+''.join(f'case {native_ids[r["rule"]]}:return {i};' for i,r in enumerate(recipes))+'default:return -1;}}\n'
    code+='uint sd_nparams(uint recipe){switch(recipe){'+''.join(params)+'default:return 0;}}\n'
    code+='uint sd_role(uint recipe,uint atom){switch(recipe){'+''.join(roles)+'default:return 0;}}\n'
    code+='template<typename R> void sd_parameters(uint recipe,thread const uint*selected,R rows,thread long*v){switch(recipe){'+''.join(fields)+'default:break;}}\n'
    code+='void sd_execute(uint recipe,thread long*v,device const long*numeric,device long*r,device uint*p,device uint*fp,device Meta&m){switch(recipe){'+''.join(effects)+'default:m.error=30;}}\n'
    return code,{'enabled':bool(program),'all_recipes':bool(recipes) and len(recipes)==len(native),'width':width,'program':program}

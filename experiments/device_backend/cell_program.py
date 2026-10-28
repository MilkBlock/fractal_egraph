"""Mechanical lowering of analyzer-exported Cell instructions, not discovery."""
import json

def lower(artifact,core,strings,joint):
    templates=[] if artifact is None else artifact['templates']
    if templates and not joint.get('program'):raise ValueError('Cell instructions require the admitted joint carrier')
    tables={t['name']:i for i,t in enumerate(core['tables'])};width=max([t['slots'] for t in templates]+[1])
    def operand(x,prefix='v'):
        if 'slot' in x:return f'{prefix}[{x["slot"]}]'
        value=x['literal']
        if isinstance(value,str):value=strings[value]
        if isinstance(value,bool):value=int(value)
        return '(-9223372036854775807L-1L)' if value==-(1<<63) else str(value)+'L'
    lines=[]
    lines+=['int cp_template(uint table){switch(table){'+''.join(f'case {tables[t["anchor"]]}:return {i};' for i,t in enumerate(templates))+'default:return -1;}}']
    cases=[];values=[];bind=[];valid=[]
    for i,t in enumerate(templates):
        ops=[(j,x) for j,s in enumerate(t['stages']) for x in s['operations']];body=[]
        for stage_id,op in ops:
            body.append(f'if(probe||stage=={stage_id}){{')
            dst=op['out'];args=op['args'];a=[operand(x) for x in args];ready='&&'.join(f'known[{x["slot"]}]' for x in args if 'slot' in x) or 'true'
            if op['op']=='construct':
                table=tables[op['table']];body.append('{long args[COLS]={'+','.join(a)+'};')
                body.append(f'if(probe){{if({ready}){{long row=row_lookup(r,p,m,{table},args);if(row>=0&&r[row*STRIDE+1])return false;known[{dst}]=row>=0;v[{dst}]=row>=0?canon(r,p,{table},WIDTH[{table}]-1,r[row*STRIDE+4+WIDTH[{table}]-1]):0;}}else known[{dst}]=false;}}')
                body.append(f'else{{v[{dst}]=ensure_constructor(r,p,fp,m,{table},args);known[{dst}]=true;}}}}')
            elif op['op']=='integer':
                code={'+':0,'-':1}[op['operator']];body.append(f'if(probe){{if(!({ready})||!integer_valid({code},{a[0]},{a[1]}))return false;v[{dst}]=integer_eval({code},{a[0]},{a[1]});known[{dst}]=true;}}')
            elif op['op']=='union':body.append(f'if(!probe)unite_now(p,{a[0]},{a[1]},m);known[{dst}]=true;v[{dst}]=0;')
            elif op['op']=='publish':body.append('{long args[COLS]={'+a[0]+'};'+f'if(!probe)v[{dst}]=write_row(r,p,fp,m,{tables[op["table"]]},args,{a[1]},false);else v[{dst}]=0;known[{dst}]=true;'+'}')
            else:raise ValueError('unknown Cell instruction')
            body.append('if(m.error)return false;}')
        cases.append(f'case {i}:{{'+''.join(body)+'return true;}')
        values.append(f'case {i}:switch(kind){{'+''.join(f'case {k}:return {operand(t["outputs"][name])};' for k,name in enumerate(['candidate','term','cost','sentinel','choice','root']))+'default:return 0;}')
        b=[];vv=[]
        for j,p in enumerate(t['parameters']):
            if p['kind']=='anchor':b.append(f'v[{j}]=r[row*STRIDE+4+{p["column"]}];known[{j}]=true;');continue
            col=p['child_column'];field={'term':0,'cost':1,'return':2}[p['kind']]
            b.append(f'{{uint key=uint(r[row*STRIDE+4+{col}]),owner=cpOwnerMap[key];long term,cost,pair;if(cpInfo[4*owner+2]==5){{uint child=uint(cpData[owner*(CP_WIDTH+1)]-1);device const long*data=cpData+owner*(CP_WIDTH+1)+1;term=cp_value(child,1,data);cost=cp_value(child,2,data);pair=cp_value(child,0,data);}}else{{uint f=fmap[key];if(!f)return false;uint id=cu_root(p,uint(r[(f-1)*STRIDE+5]));if(counts[id]!=1)return false;uint witness=items[offsets[id]];term=r[witness*STRIDE+4];cost=r[witness*STRIDE+5];pair=id;}}v[{j}]='+['term','cost','pair'][field]+f';known[{j}]=true;}}')
            if p['kind']=='return':vv.append(f'{{uint key=uint(data[{col}]),f=fmap[key];if(!f||cu_root(p,uint(r[(f-1)*STRIDE+5]))!=cu_root(p,uint(data[{j}]))||counts[cu_root(p,uint(data[{j}]))]!=1)return false;}}')
        bind.append(f'case {i}:{{'+''.join(b)+'return true;}')
        vv=['if(state==5&&phase==0){'+''.join(vv)+'}']
        vv.append('uint rootid=uint(cp_value(t,5,data)),f=fmap[rootid],pair=cu_root(p,uint(cp_value(t,0,data)));if(counts[pair]!=1)return false;if(state==2){if(f)return false;}else{uint sentinel=cu_root(p,uint(cp_value(t,3,data)));if(counts[sentinel]!=1)return false;if(state==3&&phase==1){if(!f||cu_root(p,uint(r[(f-1)*STRIDE+5]))!=sentinel)return false;}if(state==5&&(!f||cu_root(p,uint(r[(f-1)*STRIDE+5]))!=pair))return false;}')
        valid.append(f'case {i}:{{'+''.join(vv)+'return true;}')
    lines.append('template<typename V> long cp_value(uint t,uint kind,V v){switch(t){'+''.join(values)+'default:return 0;}}')
    lines.append('bool cp_evaluate(uint t,thread long*v,thread bool*known,bool probe,uint stage,device long*r,device uint*p,device uint*fp,device Meta&m){switch(t){'+''.join(cases)+'default:return false;}}')
    sig='uint t,uint row,device long*r,device uint*p,device const uint*fmap,device const uint*counts,device const uint*offsets,device const uint*items,device const uint*cpOwnerMap,device const uint*cpInfo,device const long*cpData,thread long*v,thread bool*known'
    lines.append('bool cp_bind_parameters('+sig+'){switch(t){'+''.join(bind)+'default:return false;}}')
    lines.append('bool cp_still_valid(uint t,device const long*data,device long*r,device uint*p,device const uint*fmap,device const uint*counts,uint state,uint phase){switch(t){'+''.join(valid)+'default:return false;}}')
    guards=[]
    for i,t in enumerate(templates):
        o=t['outputs'];slots=[o[k]['slot'] for k in ['candidate','sentinel','choice']];hi=t['guards']['cost_interval'][1]
        guards.append(f'case {i}:{{long cost={operand(o["cost"])};if(cost<0||cost>={hi}L)return false;'+''.join(f'if(known[{s}]&&!knownCosts[cu_root(p,uint(v[{s}]))]&&counts[cu_root(p,uint(v[{s}]))]!=1)return false;' for s in slots[:2])+f'if(known[{slots[2]}]&&(!known[{slots[0]}]||cu_root(p,uint(v[{slots[2]}]))!=cu_root(p,uint(v[{slots[0]}]))))return false;return true;}}')
    lines.append('bool cp_guard(uint t,thread long*v,thread bool*known,device uint*p,device const uint*counts,device const uint*knownCosts){switch(t){'+''.join(guards)+'default:return false;}}')
    lines.append('uint cp_operation_count(uint t,uint stage){switch(t){'+''.join(f'case {i}:{{const uint n[]={{'+','.join(str(len(s['operations'])) for s in t['stages'])+'};return n[stage];}' for i,t in enumerate(templates))+'default:return 0;}}')
    return '\n'.join(lines),{'enabled':bool(templates),'artifact_sha256':artifact['sha256'] if artifact else None,'width':width,'templates':templates}

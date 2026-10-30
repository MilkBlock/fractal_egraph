"""Read-only context validation -> keyed effects -> resolved local continuations.

Admission is per recipe, not per ruleset. Additive constructor/union effects and
pure min/max functions commute across contexts. Other merges retain the ordered
publisher. No match, binding, or candidate budget is introduced.
"""
MINMAX={'(min old new)','(min new old)','(max old new)','(max new old)'}


def eligible(template,recipe):
    tables={t['name']:t for t in recipe['tables']}
    for step in template['steps']:
        r=recipe['rules'][step['recipe_rule']]
        if any(s['sort']['kind'] in ('set','f64') for s in r['slots']):return False
        for e in r['effects']:
            k=e['kind']
            if k in ('copy','compute','union'):continue
            if k not in ('construct','set'):return False
            t=tables[e['call']['table']]
            if t['kind']=='constructor':
                if t.get('merge') is not None:return False
            elif k!='set' or not (t.get('merge') in MINMAX and t['schema'][-1]['kind']=='i64' or t.get('merge')=='old' and t['schema'][-1]['kind']=='Unit'):return False
            if k=='set' and any('table' in m['call'] for rec in step['receipts'] if rec['effect']==r['effects'].index(e) for m in rec['merge']):return False
    return True


def lower(templates,recipe,table_ids,string_ids):
    from .compiler import integer_op,boolean_expr
    def literal(x):
        n=string_ids[x['literal']] if x['sort']['kind']=='String' else int(x['literal'])
        return '(-9223372036854775807L-1L)' if n==-(1<<63) else str(n)+'L'
    def value(x):return f'o[{x["slot"]}]' if 'slot' in x else literal(x)
    def mapped(step,x):return step['mapping'][x['slot']] if 'slot' in x else x
    def ready(xs):return '&&'.join(f'o[BC_WIDTH+{x["slot"]}]' for x in xs if 'slot' in x) or 'true'
    def assign(x,v):return f'o[{x["slot"]}]={v};o[BC_WIDTH+{x["slot"]}]=1;'
    def primitive(call,args):
        code=integer_op(call)
        if code is not None:return f'integer_valid({code},{args[0]},{args[1]})',f'integer_eval({code},{args[0]},{args[1]})'
        e=boolean_expr(call,args)
        if e is not None:return 'true',e
        p=call['primitive']
        if p=='guard':return f'bool({args[0]})','0L'
        if p in ('<','>','<=','>=','!='):return f'({args[0]}{p}{args[1]})','0L'
        raise ValueError('unsupported wave primitive '+p)
    code=['''
uint cw_root(device const uint*p,uint x){while(p[x]!=x)x=p[x];return x;}
long cw_canon(device const uint*p,uint t,uint col,long x){return EQ[t*COLS+col]==1?long(cw_root(p,uint(x))):x;}
void cw_add(device long*s,uint at,uint n=1){uint old=atomic_fetch_add_explicit((device atomic_uint*)(s+at),n,memory_order_relaxed);if(old>0xffffffffu-n)atomic_fetch_add_explicit((device atomic_uint*)(s+at)+1,1u,memory_order_relaxed);}
'''];cases=[];responses=[];accepted=[];cert_cases=[]
    hidden={e['call']['table'] for rr in recipe['rules'] for e in rr['effects'] if e['kind'] in ('subsume','delete')}
    for ti,t in enumerate(templates):
        if not eligible(t,recipe):continue
        accepted.append(ti);lines=['out[0]=-1;if(o[2*BC_WIDTH]==0&&o[BC_FAST]>0)cw_add(stats,7);']
        if len(t['steps'])==1 and not any(e.get('call',{}).get('table') in hidden for e in recipe['rules'][t['steps'][0]['recipe_rule']]['effects']):
            lines.append('if(o[BC_FAST]<0){if(o[2*BC_WIDTH]!=1){o[2*BC_WIDTH]=1;cw_add(stats,1);cw_add(stats,28);cw_add(stats,34);}return;}')
        reply=[]
        for si,step in enumerate(t['steps']):
            rule=recipe['rules'][step['recipe_rule']];state=f'o[2*BC_WIDTH+{si}]'
            deps='&&'.join(f'o[2*BC_WIDTH+{d}]==1' for d in step['requires']) or 'true'
            lines.append(f'do{{if({state}==1||!({deps}))break;bool pass=true;if({state}<3){{')
            if si:
                known={x['slot'] for a in rule['query'] if 'table' in a['call'] for x in a['args'] if 'slot' in x}
                for a in rule['query']:
                    xs=[mapped(step,x) for x in a['args']];vs=list(map(value,xs))
                    if 'table' in a['call']:
                        tid=table_ids[a['call']['table']]
                        keys=','.join(f'cw_canon(p,{tid},{j},{v})' for j,v in enumerate(vs[:-1])) or '0'
                        lines.append(f'if(!({ready(xs)}))pass=false;else{{long key[COLS]={{{keys}}};long row=query_lookup(r,{tid},key);cw_add(stats,51);if(row<0||r[row*STRIDE+1]!=0||cw_canon(p,{tid},WIDTH[{tid}]-1,r[row*STRIDE+4+WIDTH[{tid}]-1])!=cw_canon(p,{tid},WIDTH[{tid}]-1,{vs[-1]}))pass=false;}}')
                    else:
                        guard,expr=primitive(a['call'],vs[:-1]);lines.append(f'if(!({ready(xs[:-1])})||!({guard}))pass=false;')
                        if 'slot' in a['args'][-1] and a['args'][-1]['slot'] not in known:
                            lines.append('if(pass){'+assign(xs[-1],expr)+'}');known.add(a['args'][-1]['slot'])
                        else:lines.append(f'if(pass&&({expr})!={vs[-1]})pass=false;')
            lines.append(f'if(!pass){{if({state}!=2)cw_add(stats,3);{state}=2;break;}}{state}=3;cw_add(stats,44);}}')
            receipts={x['effect']:x for x in step['receipts']}
            from eggfuse.compiler.recipes.constructor_witnesses import constructor_witnesses
            witnessed=constructor_witnesses(rule,recipe['tables'])
            for ei,e in enumerate(rule['effects']):
                lines.append(f'if({state}<={ei+3}){{')
                xs=[mapped(step,x) for x in e.get('args',[])];vs=list(map(value,xs));k=e['kind']
                if ei in witnessed and e['call']['table'] not in hidden:
                    known='long(cw_root(p,uint('+value(mapped(step,witnessed[ei]['value']))+')))'
                    tid=table_ids[e['call']['table']]
                    keys=','.join(f'cw_canon(p,{tid},{j},{v})' for j,v in enumerate(vs)) or '0'
                    lines.append(f'if(stats[14]){{long key[COLS]={{{keys}}};long row=query_lookup(r,{tid},key);cw_add(stats,23);if(row<0||cw_canon(p,{tid},WIDTH[{tid}]-1,r[row*STRIDE+4+WIDTH[{tid}]-1])!={known}){{out[0]=-3;out[9]=36;return;}}}}')
                    if k=='construct':
                        lines.append(assign(mapped(step,{'slot':e['dst']}),known)+f'{state}={ei+4};cw_add(stats,4);cw_add(stats,40);'+'}')
                        continue
                    # A matched immutable constructor already supplies the key
                    # and result. Its set is just an equality request.
                    rc=receipts[ei];other=value(mapped(step,e['value']))
                    response=assign(rc['before'],'out[7]')+assign(rc['actual'],'out[7]')+f'{state}={ei+4};cw_add(stats,4);cw_add(stats,18);cw_add(stats,20);cw_add(stats,41);'
                    lines.append(f'out[0]=-2;out[1]=3;out[2]=0;out[3]={known};out[4]=cw_root(p,uint({other}));out[5]={si};out[6]={ei+4};out[7]=out[3];')
                    lines.append('if(out[3]==out[4]){'+response+'out[0]=-1;cw_add(stats,54);}else{cw_add(stats,45);return;}'+f'{state}={ei+4};'+'}')
                    reply.append(f'if(step=={si}&&next=={ei+4}){{'+response+'return;}')
                    continue
                if k=='copy':lines.append(assign(mapped(step,{'slot':e['dst']}),value(mapped(step,e['arg']))))
                elif k=='compute':
                    cond,expr=primitive(e['call'],vs)
                    dest=mapped(step,{'slot':e['dst']})
                    lines.append(f'if(stats[14]){{cw_add(stats,8);if(o[BC_WIDTH+{dest["slot"]}]&&o[{dest["slot"]}]!=({expr})){{out[0]=-3;out[9]=33;return;}}}}if(!o[BC_WIDTH+{dest["slot"]}]){{if(!({cond})){{out[0]=-3;out[9]=5;return;}}'+assign(dest,expr)+'}')
                else:
                    tid=table_ids[e['call']['table']] if k!='union' else -2
                    lines.append(f'out[0]={tid};out[1]={0 if k=="construct" else 1 if k=="set" else 2};out[2]=-1;out[4]=0;out[5]={si};out[6]={ei+4};out[7]=-1;')
                    if k=='union':lines.append(f'out[3]=cw_root(p,uint({vs[0]}));out[4]=cw_root(p,uint({vs[1]}));')
                    else:
                        lines.extend(f'out[12+{j}]=cw_canon(p,{tid},{j},{v});' for j,v in enumerate(vs))
                        if k=='set':lines.append(f'out[3]=cw_canon(p,{tid},WIDTH[{tid}]-1,{value(mapped(step,e["value"]))});')
                        lines.append(f'long key[COLS];for(uint j=0;j<WIDTH[{tid}]-1;j++)key[j]=out[12+j];out[2]=query_lookup(r,{tid},key);if(out[2]>=0)out[7]=cw_canon(p,{tid},WIDTH[{tid}]-1,r[uint(out[2])*STRIDE+4+WIDTH[{tid}]-1]);')
                    site=len(lines);lines.append('')
                    response=[]
                    if k=='construct':response.append(assign(mapped(step,{'slot':e['dst']}),'out[4]'))
                    elif k=='set':
                        rc=receipts[ei];response.append(f'o[BC_WIDTH+{rc["before"]["slot"]}]=out[2]>=0;')
                        response.append(f'if(out[2]>=0)o[{rc["before"]["slot"]}]=out[7];'+assign(rc['actual'],'out[4]'))
                        for re in rc['merge']:
                            vals=list(map(value,re['args']));cond,expr=primitive(re['call'],vals)
                            response.append(f'o[BC_WIDTH+{re["dst"]["slot"]}]=0;if(({ready(re["args"])})&&({cond})){{'+assign(re['dst'],expr)+'}')
                    response.append(f'{state}={ei+4};cw_add(stats,4);')
                    reply.append(f'if(step=={si}&&next=={ei+4}){{'+''.join(response)+'return;}')
                    if k=='construct':empty='out[2]>=0'
                    elif k=='union':empty='out[3]==out[4]'
                    else:empty=f'out[2]>=0&&((KIND[{tid}]==0&&out[3]==out[7])||(POLICY[{tid}]==1&&out[3]>=out[7])||(POLICY[{tid}]==2&&out[3]<=out[7])||(POLICY[{tid}]==4&&out[3]==out[7]))'
                    lines[site]=f'if({empty}){{'+('out[4]=out[7];' if k!='union' else '')+''.join(response)+'out[0]=-1;cw_add(stats,54);}else{cw_add(stats,45);return;}'

                lines.append(f'{state}={ei+4};}}')
            lines.append(f'{state}=1;cw_add(stats,{1 if si==0 else 2});}}while(false);')
        code.append(f'__attribute__((noinline)) void cw_plan_{ti}(device long*o,device long*out,device long*r,device const uint*p,device long*stats){{'+''.join(lines)+'}')
        code.append(f'void cw_reply_{ti}(device long*o,device long*out,device long*stats){{uint step=uint(out[5]),next=uint(out[6]);'+''.join(reply)+'}')
        from eggfuse.ir.batches import certifiable
        hidden={e['call']['table'] for rr in recipe['rules'] for e in rr['effects'] if e['kind'] in ('subsume','delete')}
        certs=[];ordinal=0
        for si,step in enumerate(t['steps']):
            rule=recipe['rules'][step['recipe_rule']]
            if not si or not certifiable(rule,recipe['tables']):continue
            at=f'(base+4*(record+{ordinal}))';ordinal+=1
            certs.append('{uint at='+at+';for(uint j=0;j<4;j++)stats[at+j]=0;')
            if any(e.get('call',{}).get('table') in hidden for e in rule['effects']):certs.append('}');continue
            atom=next(a for a in rule['query'] if 'table' in a['call']);tid=table_ids[atom['call']['table']]
            xs=[mapped(step,x) for x in atom['args']];vs=list(map(value,xs))
            keys=','.join(f'cw_canon(p,{tid},{j},{v})' for j,v in enumerate(vs[:-1])) or '0'
            certs.append(f'if(o[2*BC_WIDTH+{si}]==1){{long key[COLS]={{{keys}}};long row=query_lookup(r,{tid},key);if(row>=0&&r[row*STRIDE+1]==0){{uint fid=uint(r[row*STRIDE+3]),rev=uint(r[RC_FACT_REV+fid]);if(rev!=0xffffffff){{stats[at]={step["rule"]};stats[at+1]=fid;stats[at+2]=rev;cw_add(stats,55);}}}}}}}}')
        cert_cases.append(f'case {ti}:{{'+''.join(certs)+'return;}')
        cases.append(f'case {ti}:cw_plan_{ti}(o,out,r,p,stats);return;')
        responses.append(f'case {ti}:cw_reply_{ti}(o,out,stats);return;')
    code.append('bool cw_eligible(int plan){switch(plan){'+''.join(f'case {i}:return true;' for i in accepted)+'default:return false;}}')
    code.append('void cw_propose(int plan,device long*o,device long*out,device long*r,device const uint*p,device long*stats){switch(plan){'+''.join(cases)+'default:out[0]=-3;out[9]=32;return;}}')
    code.append('void cw_reply(int plan,device long*o,device long*out,device long*stats){switch(plan){'+''.join(responses)+'default:return;}}')
    code.append('void cw_certificates(int plan,device long*o,device long*r,device const uint*p,device long*stats,uint record,uint base){switch(plan){'+''.join(cert_cases)+'default:return;}}')
    return '\n'.join(code),dict(eligible_templates=accepted,contract='frozen read-only step validation; correlated effects; constructor/union and pure min/max keyed merge; custom effects remain ordered')

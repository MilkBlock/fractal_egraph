"""Compile a shared context matrix into speculative arithmetic and full effects.

Integer/identity columns stay exact. Bounded affine columns may use FP32 SIMD
matrix multiplication; every intermediate affine bound is checked at compile
 time. Stable block publication validates each successor against actual rows.
"""
from eggfuse.ir.recipe import build,digest
from eggfuse.ir.batches import validate_batch

BOUND=256
EXACT_BOUND=1<<22


def lower(core,regions,string_ids):
    from .compiler import integer_op,boolean_expr
    recipe=build({k:v for k,v in core.items() if k!='cell_input_constants'},regions['scope'])
    batch=regions['batch_program'];validate_batch(batch,recipe,regions)
    ts={t['name']:i for i,t in enumerate(core['tables'])}
    templates=batch['templates'];W=max([len(t['slots']) for t in templates]+[1]);S=max([len(t['steps']) for t in templates]+[1]);stride=2*W+S+1
    def lit(v):
        n=string_ids[v['literal']] if v['sort']['kind']=='String' else int(v['literal'])
        return '(-9223372036854775807L-1L)' if n==-(1<<63) else str(n)+'L'
    def op(v,arr='v'):return f'{arr}[{v["slot"]}]' if 'slot' in v else lit(v)
    def mapped(step,v):return step['mapping'][v['slot']] if 'slot' in v else v
    def primitive(call,args):
        code=integer_op(call)
        if code is not None:return f'integer_valid({code},{args[0]},{args[1]})',f'integer_eval({code},{args[0]},{args[1]})'
        e=boolean_expr(call,args)
        if e is not None:return 'true',e
        if call['primitive']=='guard':return f'bool({args[0]})','0L'
        if call['primitive'] in ('<','>','<=','>=','!='):return f'({args[0]}{call["primitive"]}{args[1]})','0L'
        raise ValueError('unsupported batch primitive '+call['primitive'])
    result=[f'#define BC_WIDTH {W}\n#define BC_STEPS {S}\n#define BC_STRIDE {stride}\n#define BC_TILE 128\n#define BC_FAST {2*W+S}\n#define BC_STATS 28\n']
    result.append("""
bool bc_check_return(uint table,thread long*key,long value,device long*r,device uint*p,device Meta&m){
 long row=row_lookup(r,p,m,table,key);if(row<0)return false;
 long actual=canon(r,p,table,WIDTH[table]-1,r[row*STRIDE+4+WIDTH[table]-1]);
 value=canon(r,p,table,WIDTH[table]-1,value);if(actual==value)return true;
 if(KIND[table]!=0)return false;
 for(uint n=p[IDCAP+1];n;n--){uint a=root(p,p[IDCAP+2+2*(n-1)]),b=root(p,p[IDCAP+3+2*(n-1)]);if((a==actual&&b==value)||(b==actual&&a==value))return true;}
 return false;
}
uint bc_hash(uint rule,uint fid){return (rule*0x9e3779b9u)^(fid*0x85ebca6bu);}
bool bc_completed(device long*c,uint rule,uint fid,uint unions,ulong visibility){
 uint cap=uint(c[9]);if(!cap)return false;uint base=BC_STATS+cap*(3+BC_STRIDE),records=base+2*cap,h=bc_hash(rule,fid)&(2*cap-1);
 for(uint probe=0;probe<2*cap;probe++,h=(h+1)&(2*cap-1)){
  uint entry=uint(c[base+h]);if(!entry)return false;uint at=records+4*(entry-1);
  if(c[at]==rule&&c[at+1]==fid){if(c[at+2]==unions&&c[at+3]==visibility){rc_count(c,12);return true;}return false;}
 }return false;
}
void bc_certify(device long*c,uint rule,uint fid,uint unions,ulong visibility){
 uint cap=uint(c[9]),base=BC_STATS+cap*(3+BC_STRIDE),records=base+2*cap,h=bc_hash(rule,fid)&(2*cap-1);
 while(c[base+h]){uint at=records+4*(uint(c[base+h])-1);if(c[at]==rule&&c[at+1]==fid){c[at+2]=unions;c[at+3]=visibility;return;}h=(h+1)&(2*cap-1);}
 uint entry=uint(c[15]);c[15]=entry+1;uint at=records+4*entry;c[at]=rule;c[at+1]=fid;c[at+2]=unions;c[at+3]=visibility;c[base+h]=entry+1;
}
void bc_reindex_certificates(device long*c,uint unions,ulong visibility){
 uint cap=uint(c[9]),base=BC_STATS+cap*(3+BC_STRIDE),records=base+2*cap,old=uint(c[15]);
 if(RC_LAZY_COMPACTION&&c[27]&&c[24]==cap&&c[25]==unions&&ulong(c[26])==visibility)return;
 if(!RC_LAZY_COMPACTION||old){
  for(uint i=0;i<2*cap;i++)c[base+i]=0;c[15]=0;
  for(uint i=0;i<old;i++){uint at=records+4*i;if(c[at+2]==unions&&c[at+3]==visibility)bc_certify(c,uint(c[at]),uint(c[at+1]),unions,visibility);}
 }
 c[24]=cap;c[25]=unions;c[26]=long(visibility);c[27]=1;
}
""")
    descriptors=[];prepare_cases=[];eval_cases=[];commit_cases=[];mma_cases=[];select=[]
    unique=[];intern={};aliases=[]
    for t in templates:
        signature=digest({k:v for k,v in t.items() if k!='scope'})
        if signature not in intern:intern[signature]=len(unique);unique.append(t)
        ti=intern[signature];aliases.append(ti)
        select.append(f'if(rule=={t["seed"]}&&bc_scope({t["scope"]+1},stack,depth))return {ti};')
    for ti,t in enumerate(unique):
        numeric=[i for i,s in enumerate(t['slots'][:t['input_width']]) if s['sort']['kind']=='i64']
        forms={slot:([int(j==i) for j in range(len(numeric))]+[0]) for i,slot in enumerate(numeric)}
        ops=[];affine={};valid_forms=set(forms)
        for si,step in enumerate(t['steps']):
            rule=recipe['rules'][step['recipe_rule']]
            # A producer of a guard result is also a deterministic numeric op.
            table_bound={x['slot'] for a in rule['query'] if 'table' in a['call'] for x in a['args'] if 'slot' in x}
            for a in rule['query']:
                if 'primitive' in a['call'] and 'slot' in a['args'][-1] and a['args'][-1]['slot'] not in table_bound:
                    dest=mapped(step,a['args'][-1]);args=[mapped(step,v) for v in a['args'][:-1]]
                    if 'slot' in dest:ops.append((dest['slot'],a['call'],args))
            for e in rule['effects']:
                if e['kind']=='copy':
                    dest=mapped(step,{'slot':e['dst']})['slot'];ops.append((dest,None,[mapped(step,e['arg'])]))
                elif e['kind']=='compute':
                    dest=mapped(step,{'slot':e['dst']})['slot'];ops.append((dest,e['call'],[mapped(step,v) for v in e['args']]))
        def form(v):
            if 'slot' in v:return forms.get(v['slot'])
            if v['sort']['kind'] in ('i64','Unit','bool'):return [0]*len(numeric)+[int(v['literal'])]
        for dest,call,args in ops:
            fs=[form(v) for v in args];f=None
            if call is None:f=fs[0]
            elif all(x is not None for x in fs):
                opcode=call['primitive']
                if opcode in ('+','-') and len(fs)==2:f=[x+(y if opcode=='+' else -y) for x,y in zip(*fs)]
                elif opcode=='*' and len(fs)==2:
                    if not any(fs[0][:-1]):f=[fs[0][-1]*v for v in fs[1]]
                    elif not any(fs[1][:-1]):f=[fs[1][-1]*v for v in fs[0]]
                elif call['output']=='Unit':f=[0]*(len(numeric)+1)
            if f is not None and sum(abs(x)*BOUND for x in f[:-1])+abs(f[-1])<=EXACT_BOUND:
                forms[dest]=f
                if call is not None and call['primitive'] in ('+','-','*') and all(v.get('slot') in valid_forms if 'slot' in v else abs(int(v['literal']))<=EXACT_BOUND for v in args):
                    affine[dest]=f;valid_forms.add(dest)
                elif call is None and ('slot' not in args[0] or args[0]['slot'] in valid_forms):valid_forms.add(dest)
            else:forms.pop(dest,None);valid_forms.discard(dest)
        outputs=sorted(affine);K=len(numeric)+1;P=len(outputs)
        if not numeric:outputs=[];P=0
        descriptors.append(dict(template=ti,seed=t['seed'],scope=t['scope'],steps=len(t['steps']),slots=len(t['slots']),
                                matrix_inputs=numeric,matrix_outputs=outputs,coefficients=[affine[d] for d in outputs],integer_input_bound=BOUND,
                                all_affine_intermediates_bound=EXACT_BOUND))
        init=''.join(f'o[{i}]=matches[binding*MSTRIDE+1+{i}];o[BC_WIDTH+{i}]=1;' for i in range(t['input_width']))
        cond='&&'.join(f'o[{i}]>=-{BOUND}&&o[{i}]<={BOUND}' for i in numeric) or 'false'
        prepare_cases.append(f'case {ti}:{{{init}o[BC_FAST]={"("+cond+")" if P else "0"};break;}}')
        evaluate=[]
        for dest,call,args in ops:
            exprs=[op(x,'o') for x in args];ready='&&'.join(f'o[BC_WIDTH+{x["slot"]}]' for x in args if 'slot' in x) or 'true'
            if call is None:condition,value='true',exprs[0]
            else:condition,value=primitive(call,exprs)
            body=f'o[BC_WIDTH+{dest}]=({ready})&&({condition});if(o[BC_WIDTH+{dest}])o[{dest}]={value};'
            if dest in outputs:body='if(!o[BC_FAST]){'+body+'}'
            evaluate.append(body)
        eval_cases.append(f'case {ti}:{{'+''.join(evaluate)+'break;}')
        commits=['device long*v=o;']
        certified=0
        def ready(values):return '&&'.join(f'o[BC_WIDTH+{x["slot"]}]' for x in values if 'slot' in x) or 'true'
        def assign(dest,expr):return f'{op(dest)}={expr};o[BC_WIDTH+{dest["slot"]}]=1;'
        for si,step in enumerate(t['steps']):
            rule=recipe['rules'][step['recipe_rule']]
            table_atoms=[a for a in rule['query'] if 'table' in a['call']]
            # Only stable idempotent RHSs receive permanent completion tickets.
            from eggfuse.ir.batches import certifiable
            can_certify=si>0 and certifiable(rule,recipe['tables'])
            certified+=int(can_certify)
            deps='&&'.join(f'o[2*BC_WIDTH+{d}]==1' for d in step['requires']) or 'true'
            commits.append(f'do{{if(o[2*BC_WIDTH+{si}]==1||!({deps}))break;bool pass=true;uint witness=0;')
            if si:
                # The block owner flushes unions once, then resumes this exact
                # context. Already committed steps never execute a second time.
                # Pending equalities cannot invalidate a positive row witness.
                # Check against current canonical rows; only unresolved steps
                # wait for the block's union/rebuild, not every other context.
                known={x['slot'] for a in rule['query'] if 'table' in a['call'] for x in a['args'] if 'slot' in x}
                for a in rule['query']:
                    args=[mapped(step,v) for v in a['args']];values=[op(v) for v in args]
                    if 'table' in a['call']:
                        tid=ts[a['call']['table']];vs=','.join(values[:-1]) or '0'
                        commits.append(f'if(!({ready(args)}))pass=false;else{{long key[COLS]={{'+vs+'};'+f'long row=row_lookup(r,p,m,{tid},key);if(row<0||r[row*STRIDE+1]!=0)pass=false;else{{')
                        if can_certify:commits.append('witness=uint(r[row*STRIDE+3]);')
                        commits.append(f'if(canon(r,p,{tid},{len(args)-1},r[row*STRIDE+4+{len(args)-1}])!=canon(r,p,{tid},{len(args)-1},{values[-1]}))pass=false;'+'}}')
                    else:
                        condition,value=primitive(a['call'],values[:-1]);commits.append(f'if(!({ready(args[:-1])})||!({condition}))pass=false;')
                        if 'slot' in a['args'][-1] and a['args'][-1]['slot'] not in known:
                            commits.append('if(pass){'+assign(args[-1],value)+'}');known.add(a['args'][-1]['slot'])
                        else:commits.append(f'if(pass&&({value})!={values[-1]})pass=false;')
                commits.append('if(!pass){if(o[2*BC_WIDTH+'+str(si)+']!=2)stats[3]++;o[2*BC_WIDTH+'+str(si)+']=2;break;}')
            receipts={x['effect']:x for x in step.get('receipts',[])}
            for ei,e in enumerate(rule['effects']):
                kind=e['kind'];args=[mapped(step,x) for x in e.get('args',[])];values=[op(x) for x in args]
                if kind=='copy':commits.append(assign(mapped(step,{'slot':e['dst']}),op(mapped(step,e['arg']))))
                elif kind=='compute':
                    dst=mapped(step,{'slot':e['dst']})['slot'];condition,expected=primitive(e['call'],values)
                    # Receipts/late inputs were not known during the first GEMM.
                    # Their exact source arithmetic executes on device here.
                    commits.append(f'if(!({condition})){{m.error=5;return;}}')
                    commits.append(f'if(stats[14]&&o[BC_WIDTH+{dst}]&&o[{dst}]!=({expected})){{m.error=33;return;}}if(stats[14])stats[8]++;')
                    commits.append(f'if(!o[BC_WIDTH+{dst}])v[{dst}]={expected};o[BC_WIDTH+{dst}]=1;')
                elif kind=='union':commits.append(f'unite(p,{values[0]},{values[1]},m);stats[4]++;if(m.error)return;')
                else:
                    tid=ts[e['call']['table']];vs=','.join(values) or '0';commits.append('{long key[COLS]={'+vs+'};')
                    if kind in ('construct','read'):
                        commits.append(assign(mapped(step,{'slot':e['dst']}),f'write_row(r,p,fp,m,{tid},key,0,true)'))
                        commits.append(f'if(m.error)return;if(stats[14]){{stats[23]++;if(!bc_check_return({tid},key,{op(mapped(step,{"slot":e["dst"]}))},r,p,m)){{m.error=36;return;}}}}')
                    elif kind=='set':
                        receipt=receipts[ei];before=receipt['before'];actual=receipt['actual']
                        commits.append(f'long previous=row_lookup(r,p,m,{tid},key);o[BC_WIDTH+{before["slot"]}]=previous>=0;if(previous>=0)'+op(before)+f'=canon(r,p,{tid},WIDTH[{tid}]-1,r[previous*STRIDE+4+WIDTH[{tid}]-1]);')
                        commits.append(assign(actual,f'write_row(r,p,fp,m,{tid},key,{op(mapped(step,e["value"]))},false)'))
                        commits.append(f'stats[18]++;stats[19]+=POLICY[{tid}]==6;stats[20]+=KIND[{tid}]==0;')
                        # Decode callback witnesses by actual complete keys;
                        # reading a missing callback node never constructs it.
                        for re in receipt['merge']:
                            rr=ready(re['args']);rv=[op(v) for v in re['args']];d=re['dst'];call=re['call']
                            commits.append(f'o[BC_WIDTH+{d["slot"]}]=0;if({rr}){{')
                            if 'table' in call:
                                mt=ts[call['table']];commits.append('long mk[COLS]={'+(','.join(rv) or '0')+'};'+f'long mr=row_lookup(r,p,m,{mt},mk);if(mr>=0&&r[mr*STRIDE+1]==0){{'+assign(d,f'canon(r,p,{mt},WIDTH[{mt}]-1,r[mr*STRIDE+4+WIDTH[{mt}]-1])')+'}')
                            else:
                                valid,expr=primitive(call,rv);commits.append(f'if({valid}){{'+assign(d,expr)+'}')
                            commits.append('}')
                    else:raise ValueError('uncertified batch action')
                    commits.append('stats[4]++;if(m.error)return;}')
            if can_certify:commits.append(f'if(!p[IDCAP+1])bc_certify(stats,{step["rule"]},witness,m.unions,ulong(r[RC_VISIBILITY]));')
            commits.append(f'o[2*BC_WIDTH+{si}]=1;stats[{2 if si else 1}]++;'+'}while(false);')
        descriptors[-1]['max_certificates']=certified
        result.append(f'__attribute__((noinline)) void bc_commit_{ti}(uint binding,device long*matches,device long*o,device long*r,device uint*p,device uint*fp,device Meta&m,device long*stats){{'+''.join(commits)+'}')
        commit_cases.append(f'case {ti}:bc_commit_{ti}(binding,matches,o,r,p,fp,m,stats);break;')
        if P:
            coef=','.join(str(affine[d][k])+'.0f' for k in range(K) for d in outputs)
            result.append(f'constant float BC_COEF_{ti}[]={{'+coef+'};')
            source=f'''
void bc_matrix_{ti}(device long*data,uint count,uint tid,threadgroup float*as,threadgroup float*bs,threadgroup float*cs){{
 uint lane=tid%32,group=tid/32;as+=group*64;bs+=group*64;cs+=group*64;
 for(uint base=0;base<BC_TILE;base+=32)for(uint col=0;col<{P};col+=8){{
  simdgroup_float8x8 acc(0.0f);
  for(uint k=0;k<{K};k+=8){{
   for(uint pos=lane;pos<64;pos+=32){{uint row=base+group*8+pos/8,input=k+pos%8;float value=0;
    if(row<count&&data[row*BC_STRIDE+BC_FAST]){{switch(input){{'''
            source+=''.join(f'case {j}:value=float(data[row*BC_STRIDE+{slot}]);break;' for j,slot in enumerate(numeric))
            source+=f'''case {len(numeric)}:value=1;break;default:break;}}}}
    as[pos]=value;uint kk=k+pos/8,cc=col+pos%8;bs[pos]=kk<{K}&&cc<{P}?BC_COEF_{ti}[kk*{P}+cc]:0;
   }}
   simdgroup_barrier(mem_flags::mem_threadgroup);simdgroup_float8x8 a,b;simdgroup_load(a,as,8);simdgroup_load(b,bs,8);simdgroup_multiply_accumulate(acc,a,b,acc);simdgroup_barrier(mem_flags::mem_threadgroup);
  }}
  simdgroup_store(acc,cs,8);simdgroup_barrier(mem_flags::mem_threadgroup);
  for(uint pos=lane;pos<64;pos+=32){{uint row=base+group*8+pos/8,c=col+pos%8;
   if(row<count&&c<{P}&&data[row*BC_STRIDE+BC_FAST]){{uint dst=0;switch(c){{'''
            source+=''.join(f'case {j}:dst={slot};break;' for j,slot in enumerate(outputs))
            source+='default:break;}data[row*BC_STRIDE+dst]=long(cs[pos]);data[row*BC_STRIDE+BC_WIDTH+dst]=1;}}simdgroup_barrier(mem_flags::mem_threadgroup);}}'
            result.append(source);mma_cases.append(f'case {ti}:bc_matrix_{ti}(data,count,tid,as,bs,cs);break;')
    result.append('bool bc_waiting(int plan,device long*o){switch(plan){'+''.join('case '+str(i)+':return '+'||'.join(f'o[2*BC_WIDTH+{j}]!=1' for j in range(len(t['steps'])))+';' for i,t in enumerate(unique))+'default:return false;}}')
    result.append('uint bc_certificate_bound(int plan){switch(plan){'+''.join(f'case {d["template"]}:return {d["max_certificates"]};' for d in descriptors)+'default:return 0;}}')
    result.append('bool bc_scope(uint id,device uint*stack,uint depth){for(uint i=0;i<depth;i++)if(stack[8*i+7]==id)return true;return false;}')
    result.append('int bc_select(uint rule,device uint*stack,uint depth){'+''.join(select)+'return -1;}')
    result.append('void bc_prepare(int plan,uint binding,device long*matches,device long*o){for(uint i=0;i<BC_STRIDE;i++)o[i]=0;switch(plan){'+''.join(prepare_cases)+'default:break;}}')
    result.append('void bc_evaluate(int plan,device long*o){switch(plan){'+''.join(eval_cases)+'default:break;}}')
    result.append('void bc_publish(int plan,uint binding,device long*matches,device long*o,device long*r,device uint*p,device uint*fp,device Meta&m,device long*stats){switch(plan){'+''.join(commit_cases)+'default:m.error=32;}}')
    result.append('void bc_matrix(int plan,device long*data,uint count,uint tid,threadgroup float*as,threadgroup float*bs,threadgroup float*cs){switch(plan){'+''.join(mma_cases)+'default:break;}}')
    return '\n'.join(result),dict(schema='eggfuse-batch-layout/v4',width=W,steps=S,stride=stride,tile=128,stats_words=28,words=28+128*(stride+9),templates=descriptors,source_template_layouts=aliases,
        soundness='seed snapshot witness and per-successor actual row/guard validation; ordered complete effects',
        reproducibility='fixed query order, seed binding ordinal, step ordinal, action ordinal',
        batch_program_sha256=batch['sha256'])

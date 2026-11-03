"""Typed independent-constructor publication from a matrix of exact bindings.

No speculative score controls validity. Mixed rulesets retain contiguous rule
order; unsafe actions stay in the original ordered executor.
"""

def lower(code,starts,widths,variables,tables,sets,parallel,deferred,enabled):
    eligible=[];functions=[];write_counts=[]
    for rid,(lo,hi) in enumerate(zip(starts,starts[1:])):
        ins=[code[i:i+5] for i in range(lo,hi,5)];writes=[x for x in ins if x[0]==3]
        safe=bool(writes) and widths[rid]>0 and all(x[0] in [0,1,3,6,7] for x in ins)
        deps={};args={}
        for op,dst,sub,k,value in ins:
            dep=deps.get(value,False) if k==1 else False
            if op==0:args[dst]=dep
            elif op==1:deps[dst]=dep
            elif op==3:
                t=tables[sub]
                if t['kind']!='constructor' or any(x['kind']=='set' for x in t['schema']) or any(args.get(i,False) for i in range(len(t['schema'])-1)):safe=False
                deps[dst]=True
            elif op==6:deps[dst]=args.get(0,False) or args.get(1,False)
            elif op==7:deps[dst]=any(args.values())
        write_counts.append(len(writes) if safe else 0)
        eligible.append(int(safe))
        if not safe or not enabled:continue
        lines=[f'long v{i}='+ (f'matches[binding*MSTRIDE+1+{i}]' if i<widths[rid] else '0L')+';' for i in range(len(variables[rid]))]
        cols=max(len(t['schema']) for t in tables);lines += [f'long a{i}=0;' for i in range(cols)]
        effect=0
        for op,dst,sub,k,literal in ins:
            val=f'matches[binding*MSTRIDE+1+{literal}]' if k==2 else f'v{literal}' if k else ('(-9223372036854775807L-1L)' if literal==-(1<<63) else str(literal)+'L')
            if op==0:lines.append(f'a{dst}={val};')
            elif op==1:lines.append(f'v{dst}={val};')
            elif op==3:
                lines.append(f'out[{effect}*BATCH_STRIDE]={sub};out[{effect}*BATCH_STRIDE+1]=-1;')
                lines.extend(f'out[{effect}*BATCH_STRIDE+{2+i}]=a{i};' for i in range(len(tables[sub]['schema'])-1))
                # No later table key depends on this output (checked above).
                # Numeric primitives cannot consume an equality-sort ID.
                lines.append(f'v{dst}=0;');effect+=1
            elif op==6:lines.append(f'if(!integer_valid({sub},a0,a1))return false;v{dst}=integer_eval({sub},a0,a1);')
            elif op==7:
                expr={0:'!a0',1:'a0&&a1',2:'a0!=a1',3:'!a0||a1'}.get(sub)
                if sub==4:
                    if k:raise ValueError('dynamic boolean OR')
                    expr='|'.join(f'a{i}' for i in range(literal)) or '0L'
                lines.append(f'v{dst}=({expr});')
        lines.append('return true;')
        functions.append(f'__attribute__((noinline)) bool batch_recipe_{rid}(uint binding,device long*matches,device long*out)'+'{'+''.join(lines)+'}')
    specs=[];set_specs=[];rank_ends=[]
    for si,use in enumerate(sets):
        records=[]
        if enabled and parallel[si] and deferred[si] and any(eligible[r] for r in use):
            start=0
            while start<len(use):
                mode=eligible[use[start]];end=start+1
                while end<len(use) and eligible[use[end]]==mode:end+=1
                records.append(len(specs));specs.append([start,end,mode]);start=end
        ends=list(range(1,len(use)+1))
        for spec_id in records:
            lo,hi,mode=specs[spec_id]
            if mode:
                for rank in range(lo,hi):ends[rank]=hi
        rank_ends.extend(ends);set_specs.append(records)
    cases=''.join(f'case {i}:return batch_recipe_{i}(binding,matches,out);' for i,x in enumerate(eligible) if x and enabled)
    source='\n'.join(functions)+'\nbool batch_recipe(uint rule,uint binding,device long*matches,device long*out){switch(rule){'+cases+'default:return false;}}\n'
    return source,{'eligible_rules':[i for i,x in enumerate(eligible) if x],'segments':specs,'set_segments':set_specs,'rank_ends':rank_ends,'eligible_mask':eligible,'write_counts':write_counts}

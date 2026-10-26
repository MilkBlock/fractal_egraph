"""Split a native query at a compiler-proven prefix, retaining correlated slots.

The queue is a scratch matrix of actual bindings, not facts and not a cache.
An overflow restarts the untouched Run with the original query. Nothing is
sampled or omitted. Prefix ordinal plus suffix ordinal restores native order.
"""

HEADER = 8  # plan, rank, variant, seed timestamp/row, prefix order, stamp, epoch

def split_query(lines, depth, mark, rid, variant, gate, width, context_id):
    cut, cut_depth, known, stamps, seed_time, seed_row = mark
    prefix_stamp = '0u'
    for s in stamps:
        prefix_stamp = f'max({prefix_stamp},{s})'
    append = ('if(atomic_load_explicit((device atomic_uint*)(context_info+1),memory_order_relaxed))return;'
              'uint ci=atomic_fetch_add_explicit((device atomic_uint*)context_info,1u,memory_order_relaxed);'
              'uint prefix_order=sequence++;'
              'if(ci>=CONTEXT_CAP){atomic_store_explicit((device atomic_uint*)(context_info+1),1u,memory_order_relaxed);return;}'
              'uint ca=ci*CSTRIDE;')
    values = [str(context_id), 'rank', str(variant), seed_time, seed_row,
              'prefix_order', prefix_stamp, 'm.epoch']
    values += [f'v[{i}]' for i in known]
    append += ''.join(f'contexts[ca+{i}]={v};' for i,v in enumerate(values))
    prefix = (f'if({gate})'+f'{{uint sequence=0;long v[{max(1,width)}]={{0}};do{{'+
              '\n'.join(lines[:cut])+append+'}'*cut_depth+'}while(false);}')
    suffix = '\n'.join(lines[cut:])
    # Prefix row temporaries no longer live across the split. The exact maximum
    # timestamp suffices for the final watermark check; pivot constraints were
    # already tested individually and remain in the prefix code.
    suffix = suffix.replace(f'matches[q+3+VARS]={seed_time};', 'matches[q+3+VARS]=contexts[ca+3];')
    suffix = suffix.replace(f'matches[q+4+VARS]={seed_row};', 'matches[q+4+VARS]=contexts[ca+4];')
    for s in stamps:
        suffix = suffix.replace(s, 'context_stamp')
    suffix = suffix.replace('uint q=item*MSTRIDE;', 'if(CONTEXT_TRAINING)context_results[lane*2+1]=1.0f;uint q=item*MSTRIDE;')
    suffix = suffix.replace('matches[q+5+VARS]=0;', 'matches[q+5+VARS]=contexts[ca+5];')
    restore = ''.join(f'v[{slot}]=contexts[ca+{HEADER+i}];' for i,slot in enumerate(known))
    body = ('uint ca=lane*CSTRIDE;if(contexts[ca+7]!=m.epoch){'
            'atomic_store_explicit((device atomic_uint*)&m.error,15u,memory_order_relaxed);return;}'
            'uint rank=uint(contexts[ca+1]),sequence=0,context_stamp=uint(contexts[ca+6]);'
            f'long v[{max(1,width)}]={{0}};'+restore+'do{'+suffix+'}'*(depth-cut_depth)+'}while(false);')
    return prefix, body, {'id':context_id,'rule':rid,'variant':variant,'slots':known,
                          'prefix_atoms':cut_depth,'suffix_atoms':depth-cut_depth,
                          'words':HEADER+len(known),'epoch_required':True}

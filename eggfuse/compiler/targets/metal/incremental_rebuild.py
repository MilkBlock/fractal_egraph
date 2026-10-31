"""Resident block summaries: union displacements select canonicalization work.

This is an exact optimization of the existing publication boundary. Bloom bits
only choose which row blocks to inspect; they never choose eclass identities.
"""
import re
from pathlib import Path
HERE = Path(__file__).resolve().parents[4] / 'engine/metal'
BLOCK = 256
WORDS = 32
HEADER = 4
STRIDE = 9 + WORDS


def reserve(p, enabled=True):
    base = p['capacity'] * p['stride'] + p['set_words'] + p['index_words']
    parent = p['id_capacity'] + p['union_words']
    blocks = (p['capacity'] + BLOCK - 1) // BLOCK
    p['index_words'] += HEADER + blocks * STRIDE
    p['union_words'] += 2 * WORDS + 1
    if max(base + HEADER + blocks * STRIDE, parent + 2 * WORDS + 1) >= 2**32:
        raise ValueError('incremental rebuild address space exhausted')
    p['incremental_rebuild'] = dict(schema='eggfuse-incremental-rebuild/v1', enabled=enabled,
        row_offset=base, parent_offset=parent, block_rows=BLOCK, bloom_words=WORDS,
        block_stride=STRIDE, header_words=HEADER, blocks=blocks,
        counters=['blocks_tested','blocks_skipped','rows_visited','rows_changed','summary_refreshes'],
        contract='complete block ID summaries; exact canonicalization on possible hits; container fallback; original duplicate/merge order')


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise ValueError('incremental rebuild source contract changed: '+old[:90])
    return text.replace(old, new)


def kernel_body(source, name, body, *, thread_index=False):
    m = re.search(r'kernel void '+name+r'\((.*?)\)\{', source, re.S)
    if m is None:
        raise ValueError('missing incremental rebuild kernel '+name)
    end = m.end(); depth = 1
    while depth:
        depth += (source[end]=='{')-(source[end]=='}'); end += 1
    args = m[1] + (',uint tid[[thread_index_in_threadgroup]]' if thread_index else '')
    return source[:m.start()]+'kernel void '+name+'('+args+'){'+body+'}\n'+source[end:]


def lower(p):
    info = p['incremental_rebuild']; src = p['shader']
    defines = f'''#define IR_BASE {info['row_offset']}
#define IR_PENDING {info['parent_offset']}
#define IR_SNAPSHOT (IR_PENDING+32)
#define IR_CONTROL (IR_SNAPSHOT+32)
#define IR_BLOCK 256
#define IR_WORDS 32
#define IR_STRIDE {STRIDE}
#define IR_HEADER {HEADER}
'''
    helpers, canonical, compact = (HERE/'incremental_rebuild.metal').read_text().split('// IR_KERNEL_BODY')
    src = replace_once(src, 'struct Meta {', helpers+'\nstruct Meta {')
    src = replace_once(src, 'void rc_fact_dirty(device long*r,uint row){',
                       'void rc_fact_dirty(device long*r,uint row){ir_dirty_row(r,row);')
    src = replace_once(src, 'p[max(x,y)]=min(x,y);', 'ir_union(p,max(x,y));p[max(x,y)]=min(x,y);')
    cas = 'if(atomic_compare_exchange_weak_explicit(parents+hi,&expected,lo,memory_order_relaxed,memory_order_relaxed)){'
    expected = 2 if p['cell_waves']['enabled'] else 1
    if src.count(cas) != expected:
        raise ValueError('new union writer needs an incremental rebuild hook')
    src = src.replace(cas,cas+'ir_union(p,hi);')
    # Freeze the current displacement set. Unions from later merge callbacks
    # accumulate in a separate pending set for the next iteration.
    needle = "for(long h=2;h<r[ROW_WORDS];h+=r[ROW_WORDS+h]+4)set_canon(r,p,h);"
    start = src.index('kernel void resident_rebuild_start(')
    end = src.index('kernel void ', start+12)
    part = src[start:end]
    part = replace_once(part,needle,needle+'''
 r[IR_BASE+2]=!r[IR_BASE]||r[IR_BASE+1]!=long(m.count)+1;
 for(uint w=0;w<IR_WORDS;w++){
  p[IR_SNAPSHOT+w]=atomic_exchange_explicit((device atomic_uint*)(p+IR_PENDING+w),0u,memory_order_relaxed);
 }
''')
    src = src[:start]+part+src[end:]
    pointers = '''
 uint instance=point.y,lane=point.x;if(instance>=cfg.x)return;
 device Meta&m=all_meta[instance];device long*r=all_rows+instance*GC_ROW_WORDS;
 device uint*p=all_uint+instance*GC_UINT_WORDS;
 device uint*control=all_uint+instance*GC_UINT_WORDS+GC_CONTROL;
 device uint*qheads=all_uint+instance*GC_UINT_WORDS+GC_QHEADS;
 device long*stats=all_batch+instance*(BC_STATS+batchCfg.x*(9+BC_STRIDE));
'''
    src = kernel_body(src,'resident_rebuild_canonical',pointers+canonical,thread_index=True)
    src = kernel_body(src,'resident_compact_store',pointers+compact,thread_index=True)
    # Once a complete repair established key uniqueness, output-only renaming
    # cannot create duplicate keys. Appended rows force ordinary regrouping.
    # This skips work, not merge order: callbacks are unchanged when needed.
    for name in ('rebuild_insert','rebuild_assign','merge_scan','merge_scatter'):
        match=re.search(r'kernel void resident_'+name+r'\(.*?\)\{',src,re.S)
        start=match.end();end=src.find('kernel void ',start)
        if end<0:end=len(src)
        part=src[start:end]
        guard='if(m.error||stats[10]!=6)return;'
        part=replace_once(part,guard,guard+'if(!r[IR_BASE+2])return;')
        src=src[:start]+part+src[end:]
    match=re.search(r'kernel void resident_merge_offsets\(.*?\)\{',src,re.S)
    start=match.end();end=src.find('kernel void ',start)
    part=src[start:end]
    part=replace_once(part,'if(lane||m.error)return;', 'if(lane||m.error)return;if(!r[IR_BASE+2]){control[7]=0;r[IR_BASE+3]+=control[1];return;}')
    src=src[:start]+part+src[end:]
    p['shader'] = defines+src

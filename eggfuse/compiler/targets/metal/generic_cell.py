"""One mechanical GPU lowering for analyzer-exported typed Cell programs.

Shares the actual eggfuse query, action, merge, union and rebuild implementation.
Each independent instance owns a private arena. No terminal-state replay and no
rule/table-name recognition. Only the launch controller is on the CPU.
"""
import copy, hashlib, json
from pathlib import Path
from .compiler import compile_program,encode_initial
from .scheduled_matrix import control

HERE = Path(__file__).resolve().parents[4]/'engine/metal'


def lower(core, capacity=256, match_capacity=1024, marker_program=None, region_program=None, patch_queries=True, parallel_sort=True, parallel_indexes=True,lazy_contexts=True,parallel_block_rebuild=True,skip_quiescent=True,scheduled_regions=True,spine_cache=False,local_certificates=True,parallel_noops=True,parallel_constructors=True,ordered_query_patch=True,cell_noops=True,compact_seeds=True,hierarchical_indexes=True):
    # Until probes are a future extension. Reject before allocating/mutating.
    batch=region_program.get('batch_program') if region_program else None
    compact_seeds=bool(compact_seeds and batch and not spine_cache)
    from eggfuse.ir.recipe import digest
    scope_ids={digest(s['control']):i+1 for i,s in enumerate(batch['scopes'])} if batch else {}
    ids = {s['name']: i for i, s in enumerate(core['rulesets'])}
    schedule = copy.deepcopy({'sequence': core['schedule']})
    checks={}
    def take_checks(s):
        if s.pop('assert',False):
            rs=next(r for r in core['rulesets'] if r['name']==s['run'])
            names={a['call']['table'] for r in rs['rules'] for a in r['head'] if a.get('call',{}).get('table') in core.get('assertion_tables',[])}
            if len(names)!=1:raise ValueError('assertion needs one explicit helper table')
            s['assert_table']=names.pop()
        if 'assert_table' in s:
            checks[id(s)]=s.pop('assert_table')
        if s.get('assert'):raise ValueError('assertion needs its explicit table identity')
        for b in s.get('sequence',[]):take_checks(b)
        if 'body' in s:take_checks(s['body'])
        if 'saturate' in s:take_checks(s['saturate'])
    # Control compiler rejects hidden assertions. Insert explicit check opcodes
    # while recomputing branch destinations, so a failed intermediate check
    # cannot be rescued by a later stage.
    def encode(s):
        code=[];depth=0;peak=0
        def emit(s):
            nonlocal depth,peak
            if 'run' in s:
                if s.get('until_run') or s.get('until'):raise ValueError('until probe requires ordinary execution')
                code.append([0,ids[s['run']],0,0])
                if id(s) in checks:
                    table=next(i for i,t in enumerate(core['tables']) if t['name']==checks[id(s)])
                    code.append([5,table,0,0])
            elif 'sequence' in s:
                for b in s['sequence']:emit(b)
            else:
                repeat='repeat' in s;n=s['repeat'] if repeat else 0
                if not isinstance(n,int) or not 0<=n<2**32:raise ValueError('invalid repeat')
                at=len(code);code.append([1 if repeat else 2,n,0,scope_ids.get(digest(s),0) if not repeat else 0]);depth+=1;peak=max(peak,depth)
                emit(s['body'] if repeat else s['saturate']);depth-=1;code.append([3,at,0,0]);code[at][2]=len(code)
        emit(s);code.append([4,0,0,0]);return dict(instructions=code,stack_depth=peak)
    take_checks(schedule)
    tape = encode(schedule) if checks or batch else control(schedule, ids)
    p = compile_program(core, capacity=capacity, match_capacity=match_capacity,
                        parallel_match=bool(batch), parallel_prepare=False,
                        parallel_rebuild=False, noop_preflight=bool(batch and parallel_noops),
                        query_patch=bool(batch and patch_queries), batch_rhs=bool(batch and parallel_constructors), split_matches=False,marker_program=marker_program,region_program=region_program,resident_contexts=bool(batch),lazy_contexts=bool(batch and lazy_contexts),spine_cache=spine_cache,local_certificates=local_certificates,ordered_query_patch=ordered_query_patch,witness_order=not ordered_query_patch,compact_seeds=compact_seeds)
    scheduled=None
    if batch and scheduled_regions:
        from .resident_schedules import prepare as prepare_schedules
        scheduled=prepare_schedules(core,p,region_program)
    p['resident_schedules']=scheduled
    p['local_certificates']=dict(enabled=local_certificates,proof='single immutable constructor witness and idempotent effects; invalidate only after a mutation of the exact witnessed row')
    p['spine_cache_execution']=dict(enabled=spine_cache,contract='same complete analyzed library and Cell recipes; false uses exact parallel residual queries instead of serial cache construction')
    if scheduled:
        by_schedule={digest(x['schedule']):x for x in scheduled['programs']}
        # A private-region gate retains a complete diagnostic fallback and
        # the original finite Repeat counts; neither path infers a fixed point.
        def private_encode(node):
            result=[];depth=0;peak=0
            def emit(x,allow=True):
                nonlocal depth,peak
                hit=by_schedule.get(digest(x)) if allow else None
                if hit is not None:
                    at=len(result);result.append([6,hit['index'],0,0]);emit(x,False);result[at][2]=len(result);return
                if 'run' in x:
                    result.append([0,ids[x['run']],0,0])
                    if id(x) in checks:result.append([5,next(i for i,t in enumerate(core['tables']) if t['name']==checks[id(x)]),0,0])
                elif 'sequence' in x:
                    for child in x['sequence']:emit(child,allow)
                else:
                    repeat='repeat' in x;at=len(result);result.append([1 if repeat else 2,x['repeat'] if repeat else 0,0,scope_ids.get(digest(x),0) if not repeat else 0]);depth+=1;peak=max(peak,depth)
                    emit(x['body'] if repeat else x['saturate'],allow);depth-=1;result.append([3,at,0,0]);result[at][2]=len(result)
            emit(node);result.append([4,0,0,0]);return dict(instructions=result,stack_depth=peak)
        tape=private_encode(schedule)
    # Preserve native min-ID representative preference *within each sort*.
    # Lexicographic Value(10) < Value(2) can spuriously refresh a seminaive row.
    anchors,arena,flat=encode_initial(core,p['stride']-4,{s:i for i,s in enumerate(p['strings'])},native_order=True)
    p.update(anchors=anchors,initial_rows=flat,initial_sets=arena)
    rows = capacity * p['stride'] + p['set_words'] + p['index_words']
    uint_fields = [('parent', p['id_capacity'] + p['union_words']),
                   ('fp', p['id_capacity']), ('watermarks', max(1, p['rules'])),
                   ('qheads', p['query_head_words']), ('qlinks', p['query_link_words']),
                   ('qprev', p['query_prev_words']), ('radix_sums',max(16,((p['query_sort_words']+65535)//65536)*16) if parallel_indexes and hierarchical_indexes else 1), ('seed_rows',2*capacity+(capacity+255)//256+2*len(p['tables']) if compact_seeds else 1), ('control', 40),
                   ('match_order', 2*match_capacity if batch else 1), ('noop', 1), ('context_info', 16)]
    if batch and parallel_indexes:p['workspace_words']=max(p['workspace_words'],2*p['query_sort_words'])
    long_fields = [('matches', p['workspace_words']),
                   ('registers', p['register_count']), ('contexts', 1)]
    layout = {}; offsets = []
    for fields, prefix in [(uint_fields, 'uint'), (long_fields, 'long')]:
        at = 0
        for name, width in fields:
            layout[name] = at
            offsets.append('device long*contexts=all_markers+instance*cfg.w;' if name=='contexts' else f'device {prefix}*{name}=all_{prefix}+instance*GC_{prefix.upper()}_WORDS+{at};')
            at += width
        layout[prefix + '_words'] = at
    layout['row_words'] = rows
    # Control frames: kind, count, body, end, iteration, changed, any_changed, spare.
    layout['state_words'] = 8 + max(1, tape['stack_depth']) * 8
    defines = f'#define GC_MATCH_ORDER {layout["match_order"]}\n#define GC_MATCHES {layout["matches"]}\n'+'\n'.join(f'#define GC_{k.upper()} {v}' for k, v in layout.items() if k.endswith('_words'))
    defines+=f'\n#define GC_CONTROL {layout["control"]}\n#define GC_QHEADS {layout["qheads"]}\n#define GC_QUERY_ALTERNATE {p["query_sort_words"]}\n'
    calls = [];cache_calls=[];parallel_calls=[];quiescent_cases=[]
    if batch:
        from eggfuse.ir.recipe import build
        recipe=build({k:v for k,v in core.items() if k!='cell_input_constants'},region_program['scope'])
    for si, rules in enumerate(p['set_rule_ids']):
        args = 'r,old,p,fp,m,matches,watermarks,set,code,registers,qheads,qlinks,control,match_order,noop,qprev,contexts,context_info,context_results,0'
        body = ''.join(f'query_{rid}({args},{rank});if(m.error)return;' for rank, rid in enumerate(rules))
        calls.append(f'case {si}:{{{body}break;}}')
        if batch:
            tests=[]
            for rid in rules:
                rr=recipe['rules'][region_program['rules'][rid]['recipe_rule']]
                reads=sorted(set(a['call']['table'] for a in rr['query'] if 'table' in a['call']))
                if not rr['metadata']['seminaive'] or not reads:tests.append('false');continue
                for name in reads:
                    table=next(i for i,t in enumerate(p['tables']) if t['name']==name)
                    tests.append(f'uint(r[RC_VERSION+{table}])<=watermarks[{rid}]')
            quiescent_cases.append(f'case {si}:return '+('&&'.join(tests) or 'true')+';')
            needs={}
            for rid in rules:
                rule=recipe['rules'][region_program['rules'][rid]['recipe_rule']]
                tables=[next(i for i,t in enumerate(p['tables']) if t['name']==a['call']['table']) for a in rule['query'] if 'table' in a['call']]
                for variant in region_program['rules'][rid]['variants']:
                    n=variant['node'];pivot=variant['pivot']
                    if n is None or not spine_cache:continue
                    if pivot is not None:gate=f'qheads[2*QBUCKETS+1+{tables[pivot]}]>watermarks[{rid}]'
                    elif rule['metadata']['seminaive'] and tables:gate='('+'||'.join(f'qheads[2*QBUCKETS+1+{t}]>watermarks[{rid}]' for t in tables)+')'
                    else:gate='true'
                    needs.setdefault(n,set()).add(gate)
            cache_calls.append(f'case {si}:{{'+''.join('if(('+('||'.join(sorted(gates)))+f')&&!rc_build_{n}(r,old,m,qheads,qlinks,contexts))return;' for n,gates in sorted(needs.items()))+'break;}')
            pcalls=[]
            for rank,rid in enumerate(rules):
                nodes={v['node'] for v in region_program['rules'][rid]['variants'] if v['node'] is not None and spine_cache}
                tests=[f'lane<uint(contexts[{16+4*n+3}])' for n in sorted(nodes)]
                seed_tables=p['query_seed_tables'][rid]
                if None in seed_tables:tests.append('lane==0xffffffff' if compact_seeds else 'lane==0')
                for name in seed_tables:
                    if name is not None:
                        tid=next(i for i,t in enumerate(p['tables']) if t['name']==name)
                        tests.append(f'(lane<m.count&&old[lane*STRIDE]=={tid}&&old[lane*STRIDE+1]==0)')
                guard='||'.join(tests) or 'lane==0'
                pcalls.append(f'if({guard}){{query_{rid}('+args.replace(',0',',lane')+f',{rank});if(m.error)return;}}')
            parallel_calls.append(f'case {si}:{{'+''.join(pcalls)+'break;}')
    text = (HERE / ('batched_cell.metal' if batch else 'generic_cell.metal')).read_text()
    if batch:
        from .batches import lower as lower_batches
        batch_code,batch_layout=lower_batches(core,region_program,{s:i for i,s in enumerate(p['strings'])})
        if scheduled:
            from .resident_schedules import metal as scheduled_metal
            p['shader']+='\n'+scheduled_metal((HERE/'scheduled_matrix.metal').read_text())
        p['shader']+='\n'+batch_code+'\nbool bc_quiescent(uint set,device long*r,device uint*watermarks){switch(set){'+''.join(quiescent_cases)+'default:return false;}}'
        p['batch_layout']=batch_layout
        p['batch_sort']=dict(schema='eggfuse-resident-sort/v1',enabled=parallel_sort,tile=256,local_capacity=1024,
            contract='same total binding order; local bitonic tiles and parallel rank merges; GPU-sized indirect dispatch')
        p['quiescent_runs']=dict(enabled=skip_quiescent,proof='every delta rule read-table version is at or below its watermark; source Run and control progress retained')
        p['cell_noop_proofs']=dict(enabled=cell_noops,contract='single-step Cell only; existing stable RHS no-op proof on frozen bindings; no mutable receipts are forwarded')
        defines+=f'#define BC_CELL_NOOPS {int(cell_noops)}\n'
        defines+=f'#define BC_SCHEDULED {int(bool(scheduled))}\n'
        defines+=f'#define BC_SKIP_QUIESCENT {int(skip_quiescent)}\n'
        defines+=f'\n#define BC_PARALLEL_SORT {int(parallel_sort)}\n#define BC_SORT_TILE 256\n#define BC_SMALL_SORT 1024\n'
        sort_passes=0;span=256
        while span<match_capacity:sort_passes+=1;span*=2
        index_base=3+(1+sort_passes if parallel_sort else 0)
        p['resident_indexes']=dict(schema='eggfuse-resident-indexes/v1',enabled=parallel_indexes,
            dispatch_base=index_base,dispatch_fields=7 if hierarchical_indexes else 6,hierarchical=hierarchical_indexes,radix_passes=p['query_radix_passes'],
            scratch='reuse match arena before matching; no persistent per-row/per-plan allocation')
        defines+=f'#define BC_PARALLEL_INDEXES {int(parallel_indexes)}\n'
        for i,name in enumerate(['ROWS','PAIRS','CLEAR','GATE','HASH_ROWS','HASH_BUCKETS']+(['HIST'] if hierarchical_indexes else [])):defines+=f'#define BC_INDEX_{name} {3*(index_base+i)}\n'
        if parallel_indexes:
            from .resident_indexes import lower as lower_indexes
            if p['query_radix_digit_bits']!=4:raise ValueError('resident query indexes require four-bit radix kernels')
            text+='\n'+lower_indexes((HERE/'query_parallel.metal').read_text(),hierarchical_indexes)
        p['resident_rebuild']=dict(schema='eggfuse-resident-rebuild/v1',enabled=parallel_block_rebuild,
            dispatch_base=index_base+(p['resident_indexes']['dispatch_fields'] if parallel_indexes else 0),
            contract='parallel canonicalization/grouping/compaction; original ordered merge callbacks; saved exact continuation')
        defines+=f'#define BC_PARALLEL_REBUILD {int(parallel_block_rebuild)}\n'
        if parallel_block_rebuild:
            from .resident_rebuild import lower as lower_rebuild
            for i,name in enumerate(['HASH','ROWS','ONE','COMPACT','FINISH']):defines+=f'#define BC_RB_{name} {3*(p["resident_rebuild"]["dispatch_base"]+i)}\n'
            text+='\n'+lower_rebuild((HERE/'rebuild_parallel.metal').read_text())
        p['resident_writes']=dict(schema='eggfuse-resident-writes/v1',enabled=parallel_constructors,
            dispatch_base=p['resident_rebuild']['dispatch_base']+(5 if parallel_block_rebuild else 0),
            eligible_rules=p['batch_rhs']['eligible_rules'],
            contract='contiguous independent constructor segments; stable first-witness IDs; other effects retain exact order')
        defines+=f'#define BC_PARALLEL_CONSTRUCTORS {int(parallel_constructors)}\n'
        if parallel_constructors:
            from .resident_writes import lower as lower_writes
            for i,name in enumerate(['BINDINGS','EFFECTS','HASH','ONE']):defines+=f'#define BC_WRITE_{name} {3*(p["resident_writes"]["dispatch_base"]+i)}\n'
            text+='\n'+lower_writes((HERE/'batch_rhs.metal').read_text())
        p['resident_seeds']=dict(schema='eggfuse-resident-seeds/v1',enabled=compact_seeds,
            dispatch_base=p['resident_writes']['dispatch_base']+(4 if parallel_constructors else 0),
            contract='exact compiled per-consumer delta gates; no sampled candidates, budgets or removed Cell programs')
        defines+=f'#define BC_COMPACT_SEEDS {int(compact_seeds)}\n'
        if compact_seeds:
            from .resident_seeds import lower as lower_seeds
            for i,name in enumerate(['SCAN','ONE']):defines+=f'#define BC_SEED_{name} {3*(p["resident_seeds"]["dispatch_base"]+i)}\n'
            text+='\n'+lower_seeds(p)
        calls=[c.replace('if(m.error)return;','if(m.error)break;') for c in calls]
    text = text.replace('// LOCAL_POINTERS', '\n'.join(offsets)).replace('// LOCAL_QUERIES', ''.join(calls)).replace('// CACHE_PREPARE', ''.join(cache_calls)).replace('// PARALLEL_QUERIES', ''.join(parallel_calls))
    p['shader'] += '\n' + defines + '\n' + text
    p['generic_cell'] = dict(schema='generic-cell-program/v1', transport_version=7 if batch else 2, checkpoint_version=1 if batch else 0, history_width=12 if batch else 8, kernel='batched_cell_prepare' if batch else 'generic_cell_advance', layout=layout, control=tape,
        program_sha256=hashlib.sha256(json.dumps(core, sort_keys=True).encode()).hexdigest(),
        contract='shared GPU contexts, witnessed saturation-local batches and deterministic block publication' if batch else 'shared global identity domain, original schedule, correlated event-versioned Spines' if region_program is not None else 'isolated exact local execution; not an authorization to skip global matches',
        queries='compiled exact residual queries plus witnessed saturation-local successors; history match counts exclude inline invocations')
    if region_program is not None:
        p['region_cache']=dict(header_words=16+4*len(region_program['nodes']),initial_words=16+4*len(region_program['nodes'])+1024,
                               growth='double capacity and resume untouched Run; no prior effects replayed')
    return p


def groups(library):
    if library.get('schema') != 'analyzer-device-cells/v1':
        raise ValueError('expected analyzer-device-cells/v1')
    units=[i['unit'] for i in library['instances']]
    if len(set(units))!=len(units):raise ValueError('duplicate instance identity')
    if any(not isinstance(u,int) or u<0 for u in units):raise ValueError('invalid instance identity')
    result = []
    for pi, program in enumerate(library['programs']):
        instances = [i for i in library['instances'] if i['program'] == pi]
        if not instances: continue
        cores = [dict(copy.deepcopy(program), initial=i['initial']) for i in instances]
        # String literals are dictionary encoded. Different input dictionaries
        # cannot share a compiled kernel until explicitly remapped.
        result.append((pi, instances, cores))
    if any(not isinstance(i['program'], int) or not 0 <= i['program'] < len(library['programs']) for i in library['instances']):
        raise ValueError('invalid instance program')
    return result

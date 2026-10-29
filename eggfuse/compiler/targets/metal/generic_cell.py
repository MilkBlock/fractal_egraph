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


def lower(core, capacity=256, match_capacity=1024, marker_program=None):
    # Until probes are a future extension. Reject before allocating/mutating.
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
                at=len(code);code.append([1 if repeat else 2,n,0,0]);depth+=1;peak=max(peak,depth)
                emit(s['body'] if repeat else s['saturate']);depth-=1;code.append([3,at,0,0]);code[at][2]=len(code)
        emit(s);code.append([4,0,0,0]);return dict(instructions=code,stack_depth=peak)
    take_checks(schedule)
    tape = encode(schedule) if checks else control(schedule, ids)
    p = compile_program(core, capacity=capacity, match_capacity=match_capacity,
                        parallel_match=False, parallel_prepare=False,
                        parallel_rebuild=False, noop_preflight=False,
                        query_patch=False, split_matches=False,marker_program=marker_program)
    # Preserve native min-ID representative preference *within each sort*.
    # Lexicographic Value(10) < Value(2) can spuriously refresh a seminaive row.
    anchors,arena,flat=encode_initial(core,p['stride']-4,{s:i for i,s in enumerate(p['strings'])},native_order=True)
    p.update(anchors=anchors,initial_rows=flat,initial_sets=arena)
    rows = capacity * p['stride'] + p['set_words'] + p['index_words']
    uint_fields = [('parent', p['id_capacity'] + p['union_words']),
                   ('fp', p['id_capacity']), ('watermarks', max(1, p['rules'])),
                   ('qheads', p['query_head_words']), ('qlinks', p['query_link_words']),
                   ('qprev', p['query_prev_words']), ('control', 40),
                   ('match_order', 1), ('noop', 1), ('context_info', 16)]
    long_fields = [('matches', p['workspace_words']),
                   ('registers', p['register_count']), ('contexts', 1)]
    layout = {}; offsets = []
    for fields, prefix in [(uint_fields, 'uint'), (long_fields, 'long')]:
        at = 0
        for name, width in fields:
            layout[name] = at
            offsets.append(f'device {prefix}*{name}=all_{prefix}+instance*GC_{prefix.upper()}_WORDS+{at};')
            at += width
        layout[prefix + '_words'] = at
    layout['row_words'] = rows
    # Control frames: kind, count, body, end, iteration, changed, any_changed, spare.
    layout['state_words'] = 8 + max(1, tape['stack_depth']) * 8
    defines = '\n'.join(f'#define GC_{k.upper()} {v}' for k, v in layout.items() if k.endswith('_words'))
    calls = []
    for si, rules in enumerate(p['set_rule_ids']):
        args = 'r,old,p,fp,m,matches,watermarks,set,code,registers,qheads,qlinks,control,match_order,noop,qprev,contexts,context_info,context_results,0'
        body = ''.join(f'query_{rid}({args},{rank});if(m.error)return;' for rank, rid in enumerate(rules))
        calls.append(f'case {si}:{{{body}break;}}')
    text = (HERE / 'generic_cell.metal').read_text()
    text = text.replace('// LOCAL_POINTERS', '\n'.join(offsets)).replace('// LOCAL_QUERIES', ''.join(calls))
    p['shader'] += '\n' + defines + '\n' + text
    p['generic_cell'] = dict(schema='generic-cell-program/v1', layout=layout, control=tape,
        program_sha256=hashlib.sha256(json.dumps(core, sort_keys=True).encode()).hexdigest(),
        contract='isolated exact local execution; not an authorization to skip global matches',
        queries='compiled exact local queries; not all LHS searches eliminated')
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

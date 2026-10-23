"""Symbolic query spines for complete scheduled programs.

The original program is kept byte-for-byte. Plans supply bindings at its native
Run frontiers; no auxiliary writes participate in saturation termination. Reuse
markers.layouts for every prefix/anchor/payload, not the two-atom source adapter.
Closure proofs and query-layout plans are deliberately different evidence kinds.
"""
import copy
import json
from pathlib import Path
from . import markers, solver, takeover as t, shared_prefix
from .terms import text


def write(path,value):path.write_text(json.dumps(value,indent=2)+'\n')
def slots(atom):return {x['Slot'] for x in atom['columns'] if 'Slot' in x}
def operand(o):
    if 'Slot' in o:return t.var(f's{o["Slot"]}')
    return dict(kind='literal',text=o['Literal'])


def context_candidate(q,plan,prefix):
    shape=q['shape'];ids=[plan['seed']]+[a for a,_ in plan['steps'][:prefix]]
    atoms=[shape['atoms'][i] for i in ids]
    params=sorted(set.union(set(),*(slots(a) for a in atoms)))
    roots=[t.call(a['table'],[operand(o) for o in a['columns'][:-1]]) for a in atoms]
    sig={a['table']:dict(input=a['sorts'][:-1],output=a['sorts'][-1],kind='constructor') for a in atoms}
    # Original result-to-slot equations are retained in the native shape. The
    # layout generator only chooses stored fields; it never proves injectivity.
    return dict(id=0,family=0,params={f's{i}':f's{i}' for i in params},roots=roots,acts=[],
                signatures=sig,param_sorts={f's{i}':shape['slots'][i][1] for i in params}),atoms


def options(q,rows,reuses=1):
    """All fixed-wiring roots, every prefix and original marker layout forms.

    Structural estimates rank layouts only, never certify completeness or choose
    which facts exist. Root row counts come from setup or an explicit profile.
    """
    seen=set()
    for p in q['plans']:
        shape=q['shape'];seed=shape['atoms'][p['seed']]
        root_rows=max(1,rows.get(seed['table'],1))
        for prefix in range(len(p['steps'])+1):
            c,atoms=context_candidate(q,p,prefix)
            for layout in markers.layouts(c):
                values=[]
                for e in layout['terms']:
                    if e['kind']=='var' and e['name'].startswith('s'):values.append(int(e['name'][1:]))
                    elif e['kind']=='var' and e['name'].startswith('r'):
                        o=atoms[int(e['name'][1:])]['columns'][-1]
                        if 'Slot' in o:values.append(o['Slot'])
                    elif e['kind']=='call':
                        # The marker's interior anchor is an actual row result.
                        for a in atoms:
                            if text(t.call(a['table']+'!',[operand(o) for o in a['columns'][:-1]]))==text(e):
                                if 'Slot' in a['columns'][-1]:values.append(a['columns'][-1]['Slot'])
                carried=sorted(set(values))
                key=(p['seed'],prefix,tuple(carried),text(layout['anchor']))
                if key in seen:continue
                seen.add(key)
                needed=set.union(set(),*(slots(a) for a in atoms))-slots(seed)
                resumes=prefix>0 and needed<=set(carried)
                # Common root/row subscriptions are real costs. Carrying the
                # complete prefix avoids repeated binding decoding. Unknown
                # fanout remains explicit in the report; this is not a time fit.
                remaining=len(p['steps'])-(prefix if resumes else 0)
                storage=len(carried)+prefix
                search=root_rows*(1+remaining)*max(1,reuses)
                update=root_rows*(1+prefix)
                program=copy.deepcopy(p);program.update(prefix_len=prefix,carried_slots=carried)
                indexes=[]
                for i,access in program['steps']:
                    if isinstance(access,dict) and 'Columns' in access:
                        indexes.append((shape['atoms'][i]['table'],access['Columns']))
                yield dict(plan=program,layout=layout,stored_slots=carried,
                    complete_prefix=resumes,context_atoms=prefix+1,anchor_table=seed['table'],
                    costs=dict(storage=storage,search=search,update=update),indexes=sorted(set(indexes)))


def select(queries,rows,output,weights,profile=None,marker_profile=None):
    groups=[]
    for q in queries:
        observed=(profile or {}).get((q['ruleset'],q['rule']),{})
        opts=[v for o in options(q,rows,observed.get('requests',1)) for v in shared_prefix.estimates(o,q,rows,observed.get('requests',1),marker_profile)]
        # Fallback stays a legal choice. Without measured work the compiler
        # selects a layout for each admitted requested query; it claims no speedup.
        fallback=None if profile is None else observed.get('candidate_rows',0)
        if fallback==0:continue
        groups.append((q,opts,fallback))
    with solver.Model('scheduled_spine_layouts') as m:
        variables=[];index_users={};objective=[];marker_users={};marker_costs={};marker_defs={}
        for qi,(q,opts,fallback) in enumerate(groups):
            xs=[]
            for oi,opt in enumerate(opts):
                x=m.addVar(vtype=solver.BINARY,name=f'q{qi}_layout{oi}');xs.append(x)
                c=opt['costs'];objective.append(x*(weights[0]*c['storage']+weights[1]*c['search']+weights[2]*c['update']))
                for index in opt['indexes']:index_users.setdefault(index,[]).append(x)
                if opt.get('shared'):
                    for required in opt['required_markers']:
                        key=required['definition']['key'];marker_users.setdefault(key,[]).append(x);marker_defs[key]=required['definition']
                        costs=marker_costs.setdefault(key,dict(storage=0,update=0))
                        for field in costs:costs[field]=max(costs[field],required['costs'][field])
            if fallback is not None:
                raw=m.addVar(vtype=solver.BINARY,name=f'q{qi}_original');xs.append(raw);objective.append(raw*weights[1]*fallback)
            m.addConstr(solver.quicksum(xs)==1,name=f'q{qi}_one_executor');variables.append(xs)
        for ii,(index,users) in enumerate(sorted(index_users.items())):
            y=m.addVar(vtype=solver.BINARY,name=f'index{ii}')
            for x in users:m.addConstr(y>=x)
            m.addConstr(y<=solver.quicksum(users))
            objective.append(y*weights[0]*max(1,rows.get(index[0],1)))
        for mi,(key,users) in enumerate(sorted(marker_users.items())):
            y=m.addVar(vtype=solver.BINARY,name=f'marker{mi}')
            for x in users:m.addConstr(y>=x)
            m.addConstr(y<=solver.quicksum(users))
            c=marker_costs[key];objective.append(y*(weights[0]*c['storage']+weights[2]*c['update']))
        m.setObjective(solver.quicksum(objective));m.optimize()
        if m.Status!=solver.OPTIMAL:raise ValueError('scheduled layout solver did not prove optimality')
        chosen=[]
        for (q,opts,fallback),xs in zip(groups,variables):
            i=next(i for i,x in enumerate(xs) if m.value(x)>.5)
            if i<len(opts):chosen.append(dict(ruleset=q['ruleset'],rule=q['rule'],**opts[i]))
        shared=[]
        for key in sorted({m['definition']['key'] for o in chosen if o.get('shared') for m in o['required_markers']}):
            users=[dict(ruleset=o['ruleset'],rule=o['rule'],direct=o['shared']['key']==key) for o in chosen if o.get('shared') and any(m['definition']['key']==key for m in o['required_markers'])]
            shared.append(dict(id=len(shared),definition=marker_defs[key],consumers=users,costs=marker_costs[key]))
        result=dict(shared_markers=shared,objective=m.ObjVal,variables=m.NumVars,constraints=m.NumConstrs,
            choices=sum(len(g[1]) for g in groups),selected=chosen,
            scope='optimal in exported root/fixed-order/prefix and original anchor/outside/all layout domain; structural work estimate, not globally optimal Join order or elapsed time')
        m.write(str(output/'scheduled-selection.lp'));m.write(str(output/'scheduled-selection.sol'))
    return result


def compile_program(a,data,root,weights):
    from .compiler import run,digest
    output=a.output;work=output/'work';source=a.source
    if a.knowledge_library:raise ValueError('scheduled mode uses --certificate-library; knowledge-library is the symbolic solver format')
    certificate=getattr(a,'certificate_library',None)
    if a.analysis_dir:
        if certificate:raise ValueError('choose analysis-dir or certificate-library')
        certificate=next((a.analysis_dir/f for f in ['certificates.egg','transfers.egg'] if (a.analysis_dir/f).is_file()),None)
        if certificate is None:raise ValueError('Analyze directory has no exported closure library; use analyze --bake-certificates')
    if certificate:
        (output/'certificates.egg').write_bytes(certificate.read_bytes())
        certificate=output/'certificates.egg'
    binary=root/'target/analyzer/release/examples/scheduled_spine'
    if not a.skip_build:run(['cargo','build','--offline','--release','--target-dir','target/analyzer','--example','scheduled_spine'])
    sets=set(data['rule_rulesets']);joint={'terms','terms-helpers','terms-helpers-helpers'}<=sets
    run([binary,source,work/'queries.json']+(['--joint'] if joint else [])+(['--certificates',certificate] if certificate else []))
    native=json.loads((work/'queries.json').read_text())
    write(output/'certificates.json',native.get('certificates'))
    queries=[q for q in native['queries'] if 'plans' in q]
    selected_names=None
    if a.query_selection:
        selected_names={(q['ruleset'],q['rule']) for q in json.loads(a.query_selection.read_text())['rules']}
        available={(q['ruleset'],q['rule']) for q in queries}
        if not selected_names<=available:raise ValueError('requested query not admitted: '+str(selected_names-available))
        queries=[q for q in queries if (q['ruleset'],q['rule']) in selected_names]
    rows=native['table_rows'];profile=None
    if a.query_profile:
        # Reuse the existing bench summarizer rather than inventing a counter.
        import sys
        sys.path.insert(0,str(root/'skills/cell-assisted-egglog-bench/scripts'))
        from hotspots import summarize
        measured=summarize(a.query_profile)
        raw_path=Path(a.query_profile);raw_path=raw_path/'report.json' if raw_path.is_dir() else raw_path
        provenance=raw_path.parent/'workflow.json'
        if provenance.exists() and json.loads(provenance.read_text()).get('snapshot_sha256')!=digest(source):
            raise ValueError('query profile source differs from scheduled program')
        raw=json.loads(raw_path.read_text());stats=raw.get('gpu',raw.get('runtime',{}))['rules']
        requests={(q['ruleset'],q['rule']):q['stats'].get('dispatches',0) for q in stats}
        profile={(q['ruleset'],q['rule']):dict(candidate_rows=q['candidate_rows'],requests=requests.get((q['ruleset'],q['rule']),1)) for q in measured['gpu']}
    if a.table_profile:
        r=json.loads(a.table_profile.read_text())
        if r.get('source_sha256') and r['source_sha256']!=digest(source):raise ValueError('table profile source mismatch')
        rows=r.get('exact_original_schema',{}).get('tables',r.get('table_rows',r.get('same_commit_verification',{}).get('tables')))
        if rows is None:raise ValueError('table profile needs exact_original_schema.tables or table_rows')
    if getattr(a,'marker_profile',None):
        provenance=a.marker_profile.parent/'workflow.json'
        if not provenance.is_file() or json.loads(provenance.read_text()).get('snapshot_sha256')!=digest(source):
            raise ValueError('marker measurements need matching source provenance')
    decision=select(queries,rows,output,weights,profile,shared_prefix.measurements(a.marker_profile) if getattr(a,'marker_profile',None) else None)
    chosen=decision['selected']
    (output/'shared-markers.egg').write_text(shared_prefix.egg(decision['shared_markers']))
    plans=dict(schema='cell-boundary-programs/v2',setup=native['preserved_commands'][:native['setup_commands']],
        rules=[dict(ruleset=q['ruleset'],rule=q['rule'],root=q['anchor_table'],plan=q['plan']) for q in chosen])
    write(output/'boundary-programs.json',plans)
    # Exact source preservation prevents reserved internal parser names, changed
    # rule installation order and auxiliary progress from affecting the program.
    (output/'program.egg').write_bytes(source.read_bytes())
    annotation=['; scheduled-symbolic-spines/v1; source runs with boundary-programs.json',
        '; No helper rules or marker writes are added to the original saturation.']
    for i,q in enumerate(chosen):
        annotation.append('; @spine '+json.dumps(dict(id=i,ruleset=q['ruleset'],rule=q['rule'],
            anchor=text(q['layout']['anchor']),payload=q['layout']['payload'],slots=q['stored_slots'],
            prefix_atoms=q['context_atoms'],proof='complete native query wiring; not RipSat closure equivalence'),ensure_ascii=False))
    (output/'spines.egg').write_text('\n'.join(annotation)+'\n')
    manifest=dict(schema='scheduled-symbolic-spines/v1',source_sha256=digest(source),
        program_sha256=digest(output/'program.egg'),joint=joint,
        commands=data['commands'],combined_rulesets=data['combined_rulesets'],
        functions={n:s for n,s in data['signatures'].items() if s['kind']!='constructor'},
        queries=[{k:v for k,v in q.items() if k!='plans'} for q in native['queries']],
        selection=decision,solver=solver.statistics(),
        validation='pending native execution; compilation is not validation',
        runtime='cell-assisted original schedule + complete query providers; optional existing joint proofs unchanged',
        analysis_source=str(a.analysis_dir) if a.analysis_dir else None,
        certificate_library=str(certificate) if certificate else None,
        certificates=native.get('certificates'),
        evidence='typed query layouts plus separately native-checked imported closures' if certificate else 'typed source query wiring; no new Analyze closure certification',
        profiles={str(p):digest(p/'report.json' if p.is_dir() else p) for p in [a.query_profile,a.table_profile,a.query_selection,getattr(a,'marker_profile',None)] if p},
        preserved_source=True,concrete_snapshot=False)
    write(output/'scheduled-plan.json',manifest)
    return dict(status='scheduled_symbolic',cellified=False,scheduled=True,source_preserved=True,
        query_count=len(native['queries']),admitted_queries=len([q for q in native['queries'] if 'plans' in q]),
        selected_queries=len(chosen),layouts_considered=decision['choices'],
        program=str(output/'program.egg'),boundary_programs=str(output/'boundary-programs.json'),
        scope=manifest['evidence'],certificate_library=str(certificate) if certificate else None,solver=manifest['solver'])

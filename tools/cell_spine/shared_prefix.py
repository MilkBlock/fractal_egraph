"""Exact alpha-normalized prefix relations and explicit, inspectable cost inputs."""
import copy
import json


def canonical(value):return json.dumps(value,sort_keys=True,separators=(',',':'))

def describe(plan):
    shape=plan['shape'];ids=[plan['seed']]+[i for i,_ in plan['steps'][:plan['prefix_len']]]
    slots=[];atoms=[]
    for i in ids:
        atom=shape['atoms'][i];columns=[]
        for operand,sort in zip(atom['columns'],atom['sorts']):
            if 'Slot' in operand:
                slot=operand['Slot']
                if slot not in slots:slots.append(slot)
                columns.append(dict(Slot=slots.index(slot),sort=sort))
            else:columns.append(dict(Literal=operand['Literal'],sort=sort))
        atoms.append(dict(table=atom['table'],columns=columns))
    body=dict(atoms=atoms,accesses=[a for _,a in plan['steps'][:plan['prefix_len']]])
    carried=[i for i,s in enumerate(slots) if s in plan['carried_slots']]
    return dict(body=body,body_key=canonical(body),carried=carried,slots=slots,
                key=canonical(dict(body=canonical(body),carried=carried)))

def estimates(opt,q,rows,reuses,measurements=None):
    root=max(1,rows.get(opt['anchor_table'],1));frames=root;required=[]
    observed_queries=(measurements or {}).get('queries',{})
    work=observed_queries.get((q['ruleset'],q['rule']))
    evaluations=work['seed_evaluations'] if work else root*max(1,reuses)
    for depth in range(opt['plan']['prefix_len']+1):
        plan=copy.deepcopy(opt['plan']);plan['prefix_len']=depth
        if depth<opt['plan']['prefix_len']:plan['carried_slots']=list(range(len(plan['shape']['slots'])))
        d=describe(plan);previous=frames
        if depth:
            access=d['body']['accesses'][-1];table=d['body']['atoms'][-1]['table']
            frames*=1 if access=='Key' else max(1,rows.get(table,0))
        observed=(measurements or {}).get('markers',{}).get(d['body_key'])
        if observed:
            frames=observed['peak_bindings'];updates=observed['lookups']+observed['candidate_rows']
            basis='measured marker edge refreshes and peak correlated bindings'
        else:
            updates=frames+previous if depth else root
            basis='current-table fanout bound, future empty tables unknown; one build estimate'
        missing=len(d['slots'])-len(d['carried'])
        # Each node is extended from its complete parent interface. Missing
        # fields retain that parent frame plus this edge's one row receipt.
        costs=dict(storage=frames*(len(d['carried'])+(2 if missing else 0))+root,update=updates)
        required.append(dict(definition=d,costs=costs,evidence=dict(basis=basis,prefix_bindings=frames,measured=bool(observed))))
    d=required[-1]['definition'];missing=len(d['slots'])-len(d['carried'])
    depth=opt['plan']['prefix_len'];steps=opt['plan']['steps'];shape=opt['plan']['shape']
    def read_work(step):
        i,access=step;arity=len(shape['atoms'][i]['columns'])
        key=arity-1 if access=='Key' else 1 if access=='Output' else access['Columns'].bit_count()
        return 1+key+arity  # read key, row lookup and binding comparisons
    residual=sum(read_work(step) for step in steps[depth:])
    output=work['emitted_bindings'] if work else 0
    shared_decode=len(d['slots'])+(len(d['body']['atoms'][-1]['columns']) if missing else 0)
    search=output+evaluations*max(1,frames/root)*(shared_decode+residual)
    shared=dict(opt,plan=dict(opt['plan'],shared_marker=True),shared=d,
        required_markers=required,costs=dict(storage=0,update=0,search=search),cost_evidence=required[-1]['evidence'])
    private=copy.deepcopy(opt);private['shared']=None
    private['costs']['storage']*=root
    root_columns=shape['atoms'][opt['plan']['seed']]['columns']
    if opt['complete_prefix'] and frames<=root:
        root_slots={c['Slot'] for c in root_columns if 'Slot' in c}
        needed=[s for s in d['slots'] if s not in root_slots]
        carried=opt['plan']['carried_slots']
        # Current PortPath resume rebuilds a required-slot set, searches the
        # selected payload and reinstalls prefix dependencies at EACH resume.
        support=sum(len(a['columns']) for a in d['body']['atoms'])+depth
        support+=sum(carried.index(s)+1 for s in needed)+len(carried)
        private_work=len(root_columns)+support+residual
    else:private_work=len(root_columns)+sum(read_work(step) for step in steps)
    private['costs']['search']=output+evaluations*private_work
    return private,shared


def measurements(path):
    r=json.loads(path.read_text());g=r.get('gpu',r.get('runtime',{}));d=g.get('saturation_diagnostics',g)
    if 'gpu' in r and not (r.get('same_commit_verification') or {}).get('exact_rows_values_equivalence_visibility'):
        raise ValueError('marker profile must have a verified native reference')
    result={}
    for marker in d.get('shared_markers',[]):
        key=canonical(json.loads(marker['body_key']));s=marker['stats']
        result[key]=dict(peak_bindings=s['peak_bindings'],lookups=s['lookups'],candidate_rows=s['candidate_rows'],root_refreshes=s['root_refreshes'])
    if not result:raise ValueError('marker profile has no measured shared prefixes')
    queries={(p['ruleset'],p['rule']):p for p in d.get('programs',[]) if p['kind']=='boundary_query'}
    return dict(markers=result,queries=queries)


def egg(definitions):
    out=['; Inspectable exact prefix relations. Event maintenance uses native commit receipts.',
         '; These helper rules are declarations of meaning, not extra scheduled scans.',
         '(ruleset cell-marker-definitions)']
    for m in definitions:
        d=m['definition'];sorts={c['Slot']:c['sort'] for a in d['body']['atoms'] for c in a['columns'] if 'Slot' in c}
        op=lambda c:f's{c["Slot"]}' if 'Slot' in c else c['Literal']
        body=[]
        for a in d['body']['atoms']:
            args=' '.join(op(c) for c in a['columns'][:-1]);call=f'({a["table"]} {args})'
            body.append(call if a['columns'][-1].get('Literal')=='()' else f'(= {op(a["columns"][-1])} {call})')
        name=f'SharedSpine{m["id"]}'
        out.append('; @marker '+json.dumps(dict(id=m['id'],body_key=d['body_key'],carried=d['carried'],consumers=m['consumers'],certificate='exact typed prefix; original consumer guards and watermarks retained')))
        out.append(f'(relation {name} ({" ".join(sorts[i] for i in d["carried"])}))')
        out.append(f'(rule ({" ".join(body)}) (({name} {" ".join(f"s{i}" for i in d["carried"])})) :ruleset cell-marker-definitions)')
    return '\n'.join(out)+'\n'

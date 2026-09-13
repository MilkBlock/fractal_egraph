"""Import witnessed Math applications into typed tier-1 templates + occurrences."""
import json
from collections import defaultdict, Counter
from pathlib import Path

def q(x):return json.dumps(str(x))
def val(v):return f'(V {q(v[0])} {q(v[1])})'
def cons(items, ctor, nil):
    out=f'({nil})'
    for x in reversed(items):out=f'({ctor} {x} {out})'
    return out

def build(profile):
    all_events={e['id']:e for e in profile['events']}
    actions=defaultdict(list)
    for w in profile['action_writes']:actions[w['match']].append(w)
    unions=defaultdict(list)
    for u in profile['unions']:
        if u['changed']:unions[u['match']].append(u)
    events={i:e for i,e in all_events.items() if e['round'] is not None and e['survived'] and e['complete_witness'] and (any(w['outcome']!='Unsupported' for w in actions[i]) or unions[i])}
    reads=defaultdict(list);schema={}
    for r in profile['reads']:
        reads[r['match']].append(r)
        if r['name'] is not None:schema[r['table']]=(r['name'],len(r['key']),r.get('column_sorts'))
    writes={w['id']:w for w in profile['committed_writes']}
    direct_by_event=defaultdict(list)
    for w in writes.values():
        if w['rebuild_of'] is None:direct_by_event[w['match']].append(w)
    verified={}
    for w in profile['witnesses']:
        for e in w['support']:verified[e['read']]=e
    def math(e,v):return ('Math',f"{e['scope']}:{v}")
    def token(w):
        name=schema.get(w['table'],(w['table'],0))[0]
        return ('Fact:'+name,f"{all_events[w['match']]['scope']}:write:{w['id']}")
    exports={};records={};reasons=Counter()
    for i,e in sorted(events.items()):
        named=sorted((k,v) for k,v in e['bindings'].items() if not k.startswith('@'))
        output=[math(e,v) for _,v in named]
        layout=[f'variable:{k}' for k,_ in named]
        for w in sorted(actions[i],key=lambda w:(w['source_span'] or '',w['table'],w['id'])):
            if w['outcome']=='Unsupported' or w['rebuild_of'] is not None:continue
            if w['table'] in schema:
                name,arity,sorts=schema[w['table']]
                if not sorts:continue
                for col,v in enumerate(w['actual'][:arity+1]):
                    output.append((sorts[col],f"{e['scope']}:{v}"));layout.append(f"{w['source_span']}:{name}:{col}")
        direct=direct_by_event[i]
        for w in sorted(direct,key=lambda w:(w['source_span'] or '',w['table'],w['id'])):
            output.append(token(w));layout.append('produced-row:'+str(w['source_span']))
        exports[i]=output
        required=[];external_facts=[];parents=set();read_links=[]
        for r in sorted(reads[i],key=lambda r:(r.get('source_span') or '',r['name'] or '',r['id'])):
            edge=verified.get(r['id']);w=writes.get(r['write']);producer=r['producer']
            valid=(edge is not None and w is not None and producer in events and producer<i and w['rebuild_of'] is None and w['actual']==r['row'] and w['id']<r['id'] and events[producer]['scope']==e['scope'])
            if valid:
                v=token(w);parents.add(producer);read_links.append((producer,v));required.append(v)
            else:
                reason='rebuild_boundary' if w and w['rebuild_of'] is not None else 'no_verified_direct_producer'
                reasons[reason]+=1
                v=('Fact:'+(r['name'] or r['table']),f"{e['scope']}:external:"+json.dumps([r['table'],r['row']],separators=(',',':')))
                external_facts.append(v);read_links.append((None,v));required.append(v)
        parents=sorted(parents);external=[];ports=[]
        wanted=[math(e,v) for _,v in named]+required
        # Binding slots followed by concrete LHS read-witness slots. The latter
        # prevents calling a comb smooth merely because all value variables match.
        for n,v in enumerate(wanted):
            preferred=read_links[n-len(named)][0] if n>=len(named) else None
            sources=[preferred] if preferred is not None else parents
            match=next(((parents.index(p),exports[p].index(v)) for p in sources if v in exports[p]),None)
            if match is not None:ports.append(('parent',*match,v[0]))
            else:
                if v not in external:external.append(v)
                ports.append(('external',external.index(v),v[0]))
        records[i]={'id':i,'rule':e['rule'],'round':e['round'],'scope':e['scope'],'parents':parents,'ports':ports,'external':external,'expected_binding':wanted,'outputs':output,'output_layout':layout,'required':required,'external_facts':external_facts,'produced':[token(w) for w in direct], 'unions':unions[i]}
    lines=['(include "experiments/tier1_effects/tier1_rule_comb_ir.egg")','(let $empty (Empty))']
    for i,r in records.items():
        parent_expr=cons([f'$c{p}' for p in r['parents']] or ['$empty'],'MoreParents','NoParents')
        coarse=any(p[0]=='external' for p in r['ports']) or not r['parents']
        slots=[]
        for p in r['ports']:
            if p[0]=='parent':
                x=f'(ParentPort {p[1]} {p[2]} {q(p[3])})';slots.append(f'(Local {x})' if coarse else x)
            else:slots.append(f'(External {p[1]} {q(p[2])})')
        binding=cons(slots,'PCons' if coarse else 'RCons','PNil' if coarse else 'RNil')
        r['kind']='CoarseComb' if coarse else 'SmoothComb'
        lines.append(f'(let $c{i} ({r["kind"]} {parent_expr} (Rule {q(r["rule"])}) {binding}))')
        lines.append(f'(let $i{i} (Occurrence {i} $c{i}))')
        for slot,p in enumerate(r['parents']):lines.append(f'(ParentAt $i{i} {slot} $i{p})')
        for relation,values in [('OutputAt',r['outputs']),('ExternalAt',r['external'])]:
            for slot,v in enumerate(values):lines.append(f'({relation} $i{i} {slot} {val(v)})')
        def effect(v):return f'(HasFact "read-row" (ACons {val(v)} (ANil)))'
        for relation,values in [('Produced',r['produced']),('ExternalFact',r['external_facts'])]:
            for v in sorted(set(map(tuple,values))):lines.append(f'({relation} $i{i} {effect(v)})')
        for u in r['unions']:lines.append(f'(Produced $i{i} (Equal {val(math(events[i],u["lhs"]))} {val(math(events[i],u["rhs"]))}))')
        lines.append(f'(Requires $i{i} {cons([effect(v) for v in r["required"]],"ECons","ENil")})')
    lines+=['(run-schedule (saturate (run tier1)))']
    for i,r in records.items():
        lines.append(f'(check (Binding $i{i} {cons([val(v) for v in r["expected_binding"]],"ACons","ANil")}))')
        lines.append(f'(check (SupportsUse $i{i} $i{i}))')
    audit={'trace_events':len(all_events),'imported_events':len(records),'excluded_events':len(all_events)-len(records),'direct_dependency_edges':sum(len(r['parents']) for r in records.values()),'external_read_reasons':dict(reasons),'changed_union_events':sum(len(x) for x in unions.values()),'kinds':dict(Counter(r['kind'] for r in records.values())),'scope':'Math-specific named-variable typing; direct row provenance verified; unsupported/rebuild origins kept as external boundaries, not invented causal edges'}
    return '\n'.join(lines)+'\n',{'audit':audit,'records':list(records.values()),'rule_dictionary':profile['rule_labels']}

if __name__=='__main__':
    import sys
    program,manifest=build(json.loads(Path(sys.argv[1]).read_text()))
    Path(sys.argv[2]).write_text(program);Path(sys.argv[3]).write_text(json.dumps(manifest,indent=2)+'\n')
    print(manifest['audit'])

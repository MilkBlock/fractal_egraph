"""Small, checked visual projection of the complete native trace."""
import json
from pathlib import Path


def export(folder,out,include_all=False):
    folder=Path(folder);events=[json.loads(s) for s in (folder/'trace.jsonl').read_text().splitlines()]
    summary=json.loads((folder/'summary.json').read_text());assert summary['output_byte_identical']
    assert any('vec_add' in s for s in summary['function']['heads'])
    nodes={(e['class'],e['node']):e for e in events if e['event']=='node'}
    def node(c,n=0):return nodes[c,n]
    def op(c,n=0):return node(c,n)['head'].split('###',1)[1]
    def label(c,n=0):
        e=node(c,n);o=op(c,n);ch=e['children']
        if o=='Bop' and op(ch[0])=='Load':
            ptr=node(ch[1])['children'];assert op(ptr[0])=='PtrAdd'
            base=node(ptr[1])['children'];assert op(ptr[1])=='Get' and op(base[0])=='Arg'
            name='a' if op(base[1])=='0' else 'b';constant=node(ptr[2])['children'][0]
            assert op(constant)=='Int';index=op(node(constant)['children'][0])
            return f'Load {name}[{index}]'
        if o=='Get':
            owner=op(ch[0]);owner=op(node(ch[0])['children'][0]) if owner=='Bop' else owner
            return f'Get({owner}, {op(ch[1])})'
        if o in ('Bop','Top'):return op(ch[0])
        return o
    reads={c:label(c,n) for c,n in nodes if op(c,n)=='Bop' and op(node(c,n)['children'][0])=='Load'}
    values={c:reads[e['children'][0]].removeprefix('Load ') for (c,n),e in nodes.items() if op(c,n)=='Get' and e['children'][0] in reads and op(e['children'][1])=='0'}
    initial=next(e for e in events if e['event']=='init');states={0:dict(id=0,position=initial['position'],cost=initial['cost'],available=initial['available'],prev=-1)}
    closures={};published={};pops=[]
    rejection=None
    for e in events:
        if e['event']=='pop':pops.append(dict(e,label=label(e['position'],states[e['id']].get('node',0))))
        if e['event']=='closure':closures[e['from'],e['target']]=e
        if e['event']=='publish':
            delta=closures[e['prev'],e['target']]
            assert all(c not in states[e['prev']]['available'] for c in delta['added'])
            states[e['id']]=dict(id=e['id'],position=e['target'],node=e['node'],cost=e['cost'],prev=e['prev'],available=sorted(set(states[e['prev']]['available'])|set(delta['added'])))
            published[e['id']]=e
        if e['event']=='candidate' and e['first_missing']>=0 and label(e['target'],e['node'])=='Write' and rejection is None:
            actual=node(e['target'],e['node'])['children'];avail=states[e['from']]['available']
            assert e['first_missing'] not in avail
            rejection=dict(e,state=dict(states[e['from']]),children=[dict(role=role,eclass=c,available=c in avail,label=label(c)) for role,c in zip(['operator','address','value','state'],actual)])
    done=summary['done'];root=next(e['root'] for e in events if e['event']=='region')
    endings=[e for e in published.values() if e['target']==root and e['cost']==done['best_cost']];assert len(endings)==1
    path=[];s=states[endings[0]['id']]
    while s['id']!=0:path.append(s);s=states[s['prev']]
    path.append(states[0]);path.reverse()
    chosen=[e for e in events if e['event']=='chosen']
    assert [(s['position'],s.get('node',0)) for s in path]==[(c['class'],c['node']) for c in chosen]
    def project(s):
        return {**{k:v for k,v in s.items() if k!='available'},'label':label(s['position'],s.get('node',0)),
                'values':[values[c] for c in sorted(values) if c in s['available']], 'available_count':len(s['available'])}
    state_returns=[s for s in path if s['position']==9 and s['id']!=1];assert len(state_returns)==6
    views=[project(s) for s in state_returns]
    assert views[0]['values']==['b[0]'] and set(views[1]['values'])=={'a[1]','b[0]'}
    first_candidates=[dict(e,label=label(e['target'],e['node']),total=1+e['incremental_estimate']) for e in events if e['event']=='candidate' and e['from']==1]
    first_write=next(s for s in path if label(s['position'],s.get('node',0))=='Write')
    accepted=project(states[first_write['prev']]);accepted['next']=project(first_write)
    assert all(c['eclass'] in states[accepted['id']]['available'] for c in rejection['children'])
    meta=dict(summary=summary,classes=next(e['classes'] for e in events if e['event']=='region'),enodes=len(nodes),state_returns=views,
              chosen=[project(s) for s in path],first_candidates=first_candidates,rejection=rejection,accepted=accepted,
              first_pops=pops[:12],
              note='Exact native local region IDs, not output __tmp names. Candidate counters count visits, not unique nodes. Pure-closure cache hits are zero in this case.')
    # Spoken numbers and IDs are deliberate claims about this frozen example.
    # Fail if the trace changes instead of silently narrating obsolete evidence.
    assert (meta['classes'],meta['enodes'],done['states'],summary['counts']['candidate'],summary['missing_dependencies'],summary['closure_cache_hits'])==(113,174,808,1864,399,0)
    assert [(v['id'],v['cost']) for v in views[:2]]==[(11,613),(73,1225)]
    assert (rejection['from'],rejection['first_missing'],accepted['id'],accepted['next']['id'])==(71,39,797,798)
    assert [p['cost'] for p in pops[:6]]==[0,1,1,2,612,612]
    if include_all:meta['state_graph']=[project(states[i]) for i in sorted(states)];meta['pops']=pops
    (out/'trace-demo.json').write_text(json.dumps(meta,ensure_ascii=False,indent=2)+'\n')
    return meta

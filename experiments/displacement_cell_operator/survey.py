"""Read an existing native Pass-3 snapshot; no Analyze or inference rerun.

Find exactly which observed Add-Gets origins admit a path-independent offset
coordinate. This measures a representation precondition, NOT Cell coverage or
runtime savings. Export a small self-contained structural slice for replay.
"""
import argparse
from collections import defaultdict, Counter
import hashlib
import json
from pathlib import Path


def eq(value): return value['Eq'][1]
def integer(value): return int(value['Base'][1])


def survey(path, output):
    with path.open() as f: saved=json.load(f)
    rows=saved['rows']
    length={eq(a[0]):integer(v) for op,a,v,hidden in rows if op=='tuple-length' and not hidden}
    concats=defaultdict(list);singles=defaultdict(set);ags=defaultdict(set)
    for op,a,v,hidden in rows:
        if hidden: continue
        if op=='Concat': concats[eq(v)].append((eq(a[0]),eq(a[1])))
        elif op=='Single': singles[eq(v)].add(eq(a[0]))
        elif op=='Add-Gets': ags[eq(a[0])].add((eq(a[1]),integer(a[2])))
    del saved,rows
    cases=[]
    for root,facts in ags.items():
        pos={root:0};queue=[root];edges=set();conflicts=[]
        while queue:
            node=queue.pop()
            for left,right in concats.get(node,[]):
                if left not in length: continue
                for child,delta in ((left,0),(right,length[left])):
                    edges.add((node,child,delta))
                    value=pos[node]+delta
                    if child not in pos: pos[child]=value;queue.append(child)
                    elif pos[child]!=value: conflicts.append([node,child,delta,pos[child]-pos[node]])
        multi=sum(count>1 for count in Counter(n for n,o in facts).values())
        cases.append({'root':root,'nodes':len(pos),'edges':len(edges),'observed_AG':len(facts),
                      'multiple_offset_nodes':multi,'consistent':not conflicts,
                      'extra_offset_rows':len(facts)-len({n for n,o in facts}),
                      'max_offsets_per_node':max(Counter(n for n,o in facts).values()),
                      'conflict':conflicts[:1], 'positions':pos,'edge_list':sorted(edges)})
    nontrivial=[x for x in cases if x['edges']]
    good=[x for x in nontrivial if x['consistent']]
    # Shared final tuple values sometimes have additional Single enodes; the
    # replayed slice retains every Concat alternative and terminal Single.
    replay=[];eligible=0
    for x in sorted(good,key=lambda x:-x['nodes']):
        nodes=set(x['positions']);done=set();order=[]
        while True:
            added=[n for n in sorted(nodes-done)
                   if all(l in done and r in done for l,r in concats.get(n,[]))]
            if not added: break
            done.update(added);order+=added
        if done!=nodes or any(n not in length or length[n]<=0 for n in nodes):continue
        if any(len(singles.get(n,()))>1 or (n in concats and n in singles) for n in nodes):continue
        if any(length[n]!=length[l]+length[r] for n in nodes for l,r in concats.get(n,[])):continue
        # All constructor alternatives must be represented, including ones
        # disabled by missing length. This isolated replay requires all known.
        if any(l not in nodes or r not in nodes for n in nodes for l,r in concats.get(n,[])):continue
        idx={n:i for i,n in enumerate(order)}
        single={idx[n]:next(iter(singles[n])) for n in nodes if n in singles}
        if len(set(single.values()))!=len(single):continue
        eligible+=1
        if len(replay)>=3: continue
        replay.append({'original_root':x['root'],'original_ids':order,'root':idx[x['root']],
                       'length':[length[n] for n in order],
                       'single':single,
                       'opaque':[idx[n] for n in nodes if n not in singles and n not in concats],
                       'concat':{idx[n]:[(idx[l],idx[r]) for l,r in concats[n]] for n in nodes if n in concats},
                       'known_length':list(range(len(order)))})
    counts={'observed_origins':len(cases),'nontrivial_origins':len(nontrivial),
            'consistent_nontrivial_origins':len(good),'inconsistent_nontrivial_origins':len(nontrivial)-len(good),
            'observed_AG_rows':sum(x['observed_AG'] for x in cases),
            'observed_AG_in_consistent_nontrivial':sum(x['observed_AG'] for x in good),
            'max_consistent_nodes':max((x['nodes'] for x in good),default=0),
            'origins_with_multiple_observed_offsets':sum(x['multiple_offset_nodes']>0 for x in cases),
            'observed_extra_offset_rows':sum(x['extra_offset_rows'] for x in cases),
            'maximum_offsets_per_node':max(x['max_offsets_per_node'] for x in cases),
            'replay_contract_eligible_roots':eligible}
    summary={'scope':'final canonical visible tuple-rule projection of a saved same-submit CPU Pass 3 snapshot; not full Cell coverage',
             'snapshot':str(path.resolve()),'snapshot_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
             'counts':counts,
             'largest_consistent':[{k:v for k,v in x.items() if k not in ('positions','edge_list')} for x in sorted(good,key=lambda x:-x['nodes'])[:5]],
             'counterexamples':[{k:v for k,v in x.items() if k not in ('positions','edge_list')} for x in nontrivial if not x['consistent']][:5],
             'replay_slices':replay}
    output.parent.mkdir(parents=True,exist_ok=True);output.write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps(counts,indent=2));print('replay_nodes', [len(x['length']) for x in replay])


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--snapshot',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();survey(a.snapshot,a.output)

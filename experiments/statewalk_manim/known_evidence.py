"""Reconstruct known sets and check liveness against Tiger's unchanged source block."""
import argparse
from collections import defaultdict, deque
import hashlib
import json
from pathlib import Path
import subprocess
from dataflow import pure_closure


def live_masks(nodes, initial, root):
    by=defaultdict(list);effectful={};parents=defaultdict(list)
    for n in nodes:by[n['class']].append(n);effectful[n['class']]=n['effectful']
    for n in nodes:
        if n['effectful']:
            for c in n['children']:
                if effectful[c]:parents[c].append(n['class'])
    masks={}
    for pos in effectful:
        if not effectful[pos]:continue
        seen=set();q=deque([pos])
        while q:
            u=q.popleft()
            if effectful[u] and u!=root:
                for v in parents[u]:
                    if v not in seen:seen.add(v);q.append(v)
            if u in seen:
                for n in by[u]:
                    for v in n['children']:
                        if v not in initial and not effectful[v] and v not in seen:
                            seen.add(v);q.append(v)
        masks[pos]={v for v in seen if not effectful[v] and v not in initial}
    return masks


def source_oracle(nodes, initial, root, source, out):
    native=source/'dag_in_context/src/tiger';text=(native/'statewalkdp.cpp').read_text()
    block=text[text.index('    // liveness\n'):text.index('    // AC - satellite eclasses')]
    assert 'liveness_delta' in block and 'parent_edge_to_effectful' in block
    count=max(n['class'] for n in nodes)+1
    lines=['#include <cstdio>','#include <cassert>','#include <iostream>','#include <queue>','#include <unordered_map>',
           '#include "egraphin.h"','#define DEBUG_ASSERT(x) assert(x)','int main() {',f'EGraph g; g.eclasses.resize({count});',f'EClassId root={root}; bool use_liveness=true;',
           f'vector<bool> init_extractable({count},false);',f'vector<int> compressed_eclass_id({count},-1);',
           f'vector<vector<pair<EClassId,ENodeId>>> parent_edge_to_effectful({count});']
    for c in sorted(initial):lines.append(f'init_extractable[{c}]=true;')
    for n in nodes:
        c=n['class'];lines.append(f'g.eclasses[{c}].isEffectful={str(n["effectful"]).lower()};')
        children=','.join(map(str,n['children']))
        lines.append(f'g.eclasses[{c}].enodes.push_back(ENode{{"",{c},{{{children}}}}});')
    lines += ['int index=0; for(int c=0;c<(int)g.neclasses();++c) {',
              'if(!init_extractable[c]) compressed_eclass_id[c]=index++;',
              'if(g.eclasses[c].isEffectful) for(int n=0;n<(int)g.eclasses[c].enodes.size();++n)',
              'for(auto u:g.eclasses[c].enodes[n].ch) if(g.eclasses[u].isEffectful) parent_edge_to_effectful[u].push_back({c,n});','}',block,
              'for(int c=0;c<(int)g.neclasses();++c) if(g.eclasses[c].isEffectful) { std::cout<<c;',
              'for(int v=0;v<(int)g.neclasses();++v) if(!init_extractable[v] && !g.eclasses[v].isEffectful && ((liveness[c][v>>6]>>(v&63))&1)) std::cout<<" "<<v;',
              'std::cout<<"\\n"; }','}']
    out.mkdir(parents=True,exist_ok=True);cpp=out/'liveness.cpp';cpp.write_text('\n'.join(lines)+'\n')
    binary=out/'liveness'
    subprocess.run(['c++','-std=c++17','-O2','-I',str(native),str(cpp),'-o',str(binary)],check=True)
    output=subprocess.check_output([str(binary)],text=True);masks={}
    for line in output.splitlines():
        pos,*values=map(int,line.split());masks[pos]=set(values)
    (out/'masks.txt').write_text(output)
    return masks,hashlib.sha256(block.encode()).hexdigest()


def export(trace, source, oracle_dir):
    events=[json.loads(line) for line in trace.read_text().splitlines()]
    nodes=[e for e in events if e['event']=='node'];init=next(e for e in events if e['event']=='init');initial=set(init['available'])
    assert pure_closure(nodes,{init['position']})[0]==initial
    root=next(e['root'] for e in events if e['event']=='region');effectful={n['class']:n['effectful'] for n in nodes}
    masks=live_masks(nodes,initial,root);native,block_sha=source_oracle(nodes,initial,root,source,oracle_dir)
    assert masks==native
    states={0:initial};closures={};records={};closure_count=0
    def record(known,pos):
        dynamic_pure={c for c in known if c not in initial and not effectful[c]}
        live=dynamic_pure&masks[pos]
        return dict(position=pos,known=sorted(known),dynamic_pure=sorted(dynamic_pure),
                    live_pure=sorted(masks[pos]),live_known=sorted(live),omitted_from_key=sorted(dynamic_pure-live),
                    known_effectful_noninitial=sorted(c for c in known if c not in initial and effectful[c]))
    for e in events:
        if e['event']=='closure':
            before=states[e['from']];after,waves=pure_closure(nodes,before|{e['target']})
            assert after-before==set(e['added']);closure_count+=1
            closures[e['from'],e['target']]=after
            if (e['from'],e['target'])==(73,21):
                detail=dict(before=sorted(before),seed=21,waves=waves,added=e['added'],after=sorted(after))
        elif e['event']=='publish':
            known=closures[e['prev'],e['target']];states[e['id']]=known
            records[e['id']]=record(known,e['target'])
    assert len(detail['before'])==52 and len(detail['after'])==63 and len(detail['added'])==11
    assert len(initial)==45 and 21 not in detail['before']
    seed_candidate=next(n for n in nodes if (n['class'],n['node'])==(21,0))
    assert seed_candidate['children']==[45,55,9] and set(seed_candidate['children'])<=set(detail['before'])
    assert len(records[200]['live_known'])==14 and not records[200]['omitted_from_key']
    assert len(records[200]['known_effectful_noninitial'])==4
    assert len(records[798]['known'])==96 and len(records[798]['live_known'])==16 and len(records[798]['omitted_from_key'])==25
    assert len(records[798]['known_effectful_noninitial'])==10
    return dict(schema='known-liveness-explainer/v1',trace_sha256=hashlib.sha256(trace.read_bytes()).hexdigest(),
                source_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip(),
                liveness_source_block_sha256=block_sha,native_liveness_masks_equal=True,
                validated_effect_positions=len(masks),validated_closures=closure_count,initial_known=sorted(initial),
                closure=detail,seed_candidate=seed_candidate,records={str(k):records[k] for k in [73,200,797,798,803,804,805,806,807]},
                scope='Known sets reconstructed and closure deltas checked against saved native trace. Liveness masks recomputed and compared with the unchanged C++ source block on that exact regional graph. No timing or collision-free native hashing claim.')


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--trace',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--oracle-dir',type=Path,required=True)
    p.add_argument('--checkout',type=Path,default=Path('/Users/mineralsteins/Repos/egg_related/check_statewalk'));a=p.parse_args()
    result=export(a.trace,a.checkout,a.oracle_dir)
    a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:result[k] for k in ['native_liveness_masks_equal','validated_effect_positions','validated_closures']}))

#!/usr/bin/env python3
"""Capture native Statewalk decisions for video, without changing the checkout."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

HERE=Path(__file__).resolve().parent
REPO=HERE.parents[1]
CHECKOUT=Path('/Users/mineralsteins/Repos/egg_related/check_statewalk')
SOURCES='egraphin.cpp greedy.cpp json2egraphin.cpp main.cpp regionalize.cpp statewalkdp.cpp tiger.cpp toegglog.cpp debug.cpp ilp.cpp time_ilp.cpp'.split()

def replace(s,a,b,count=1):
    assert s.count(a)==count,(a,s.count(a));return s.replace(a,b)

def main():
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--function-index',type=int,default=2);a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);source=out/'source';source.mkdir()
    native=CHECKOUT/'dag_in_context/src/tiger'
    for f in native.iterdir():
        if f.suffix in ('.cpp','.h'):shutil.copyfile(f,source/f.name)
    shutil.copyfile(HERE/'trace.h',source/'trace.h')
    f=source/'regionalize.cpp';s=f.read_text();s='#include "trace.h"\n'+s
    s=replace(s,'EClassId fun_root = fun_roots[i];','EClassId fun_root = fun_roots[i];\n        film_trace::function(i,g,fun_root);');f.write_text(s)
    f=source/'statewalkdp.cpp';s=f.read_text();s='#include "trace.h"\n'+s
    s=replace(s,'    // find arg','    film_trace::begin(g,root);\n    // find arg')
    s=replace(s,'    // main DP data structures','''    if (film_trace::active()) {
        vector<int> initial;for(int i=0;i<(int)init_extractable.size();i++)if(init_extractable[i])initial.push_back(i);
        film_trace::event("\\"event\\":\\"init\\",\\"position\\":"+std::to_string(argc)+",\\"cost\\":"+std::to_string(init_cost)+",\\"available\\":"+film_trace::array(initial));
    }
    // main DP data structures''')
    s=replace(s,'        maxheap.pop();','        maxheap.pop();\n        film_trace::pop(uid,dp[uid].ec,c,maxheap.size(),dp[uid].c!=c);')
    s=replace(s,'                bool can_extend = true;','                bool can_extend = true; int video_missing=-1; bool video_cache_hit=false;')
    s=replace(s,'                        can_extend = false;','                        can_extend = false; video_missing=chc;')
    s=replace(s,'                if (can_extend) {','                film_trace::candidate(uid,v,vn,video_missing,statewalk_cost[v][vn]);\n                if (can_extend) {')
    s=replace(s,'                        if (pure_saturation_cache.count(pschash)) {','''                        if (pure_saturation_cache.count(pschash)) {
                            video_cache_hit=true;
                            film_trace::event("\\"event\\":\\"cache_hit\\",\\"from\\":"+std::to_string(uid)+",\\"target\\":"+std::to_string(v));''')
    s=replace(s,'                    // update','''                    if(film_trace::active()) {
                        vector<int> added;
                        for(size_t z=0;z<inv_compressed_eclass_id.size();z++)
                            if(!true_extractable_pool.getpos(dp[uid].root,z) && true_extractable_pool.getpos(nroot,z))added.push_back(inv_compressed_eclass_id[z]);
                        film_trace::event("\\"event\\":\\"closure\\",\\"from\\":"+std::to_string(uid)+",\\"target\\":"+std::to_string(v)+",\\"cache_hit\\":"+(video_cache_hit?"true":"false")+",\\"added\\":"+film_trace::array(added));
                    }
                    // update''')
    s=replace(s,'                        maxheap.push(make_pair(~nc, vid));','                        film_trace::publish(vid,uid,v,vn,nc);\n                        maxheap.push(make_pair(~nc, vid));',2)
    s=replace(s,'    DEBUG_ASSERT(is_valid_statewalk(g, root, sw));','''    if(film_trace::active()) {
        for(auto it=sw.rbegin();it!=sw.rend();++it)film_trace::event("\\"event\\":\\"chosen\\",\\"class\\":"+std::to_string(it->first)+",\\"node\\":"+std::to_string(it->second));
        film_trace::event("\\"event\\":\\"done\\",\\"states\\":"+std::to_string(dp.size())+",\\"best_cost\\":"+std::to_string(dp[best_statewalk].c));
    }
    DEBUG_ASSERT(is_valid_statewalk(g, root, sw));''');f.write_text(s)
    subprocess.run(['c++','-O3','-std=c++17','-Wno-unused-result',*SOURCES,'-o',str(out/'tiger')],cwd=source,check=True)
    graph=REPO/'out/boundary-loop/graphs/gpu.json';pretty=out/'input.json'
    with graph.open() as f:data=json.load(f)
    with pretty.open('w') as f:json.dump(data,f,indent=2);f.write('\n')
    del data
    env=dict(os.environ,STATEWALK_VIDEO_TRACE=str(out/'trace.jsonl'),STATEWALK_VIDEO_FUNCTION=str(a.function_index))
    with pretty.open('rb') as f,(out/'output.egg').open('wb') as o,(out/'stderr').open('wb') as e:
        subprocess.run([str(out/'tiger'),'--ilp-solver','cbc'],stdin=f,stdout=o,stderr=e,env=env,check=True)
    expected=(REPO/'out/boundary-loop/extraction/gpu.egg').read_bytes();assert (out/'output.egg').read_bytes()==expected
    events=[json.loads(s) for s in (out/'trace.jsonl').read_text().splitlines()]
    summary=dict(function_index=a.function_index,function=events[0],counts={kind:sum(e['event']==kind for e in events) for kind in sorted({e['event'] for e in events})},
                 missing_dependencies=sum(e['event']=='candidate' and e['first_missing']>=0 for e in events),
                 closure_cache_hits=sum(e['event']=='cache_hit' for e in events),
                 output_byte_identical=True,input_sha256=hashlib.sha256(graph.read_bytes()).hexdigest(),trace_sha256=hashlib.sha256((out/'trace.jsonl').read_bytes()).hexdigest(),
                 done=next(e for e in events if e['event']=='done'),scope='Complete diagnostic Statewalk trace of one real region; no timing claim')
    (out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');pretty.unlink();print(json.dumps(summary,indent=2))

if __name__=='__main__':main()

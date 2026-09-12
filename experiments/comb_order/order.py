"""Rank and greedily partition the native tier-1 Comb template DAG."""
from collections import defaultdict
from fractions import Fraction
import hashlib
import heapq
import html
import json
from pathlib import Path
import argparse


def key(row):
    return (row['scope'],row['op'],tuple(zip(row['sorts'][:-1],row['values'][:-1])))


def capture(profile,manifest,tier1):
    events={r['id']:r for r in manifest['records']}
    native={i['event']:i['template'] for i in tier1['instances']}
    assert set(events)==set(native)
    schemas={}
    for r in profile['reads']:
        if r['name'] and r.get('column_sorts'):
            schemas[r['table']]=(r['name'],r['column_sorts'])
    support={s['read']:s for w in profile['witnesses'] for s in w['support']}
    writes={w['id']:w for w in profile['committed_writes']}
    rs=defaultdict(list);ws=defaultdict(list);skipped=0
    for r in profile['reads']:
        if r['match'] not in events or r['table'] not in schemas:continue
        op,sorts=schemas[r['table']]
        if sorts[-1]!='Math':continue
        producer=None;s=support.get(r['id']);w=writes.get(r['write'])
        if s and w and w['rebuild_of'] is None and w['actual']==r['row'] and w['id']<r['id'] and r['producer'] in events:
            if events[r['producer']]['scope']==events[r['match']]['scope']:producer=r['producer']
        rs[r['match']].append({'scope':events[r['match']]['scope'],'op':op,'sorts':sorts,'values':r['row'][:len(sorts)],'producer':producer,'read':r['id'],'producer_sites':s['producer_sites'] if s else [],'consumer_sites':s['consumer_sites'] if s else []})
    for w in profile['action_writes']:
        if w['match'] not in events or w['outcome']=='Unsupported' or w['rebuild_of'] is not None:continue
        if w['table'] not in schemas:skipped+=1;continue
        op,sorts=schemas[w['table']]
        if sorts[-1]!='Math':continue
        ws[w['match']].append({'scope':events[w['match']]['scope'],'op':op,'sorts':sorts,'values':w['actual'][:len(sorts)],'write':w['id'],'outcome':w['outcome']})
    records=[{'id':i,'template':native[i],'rule':r['rule'],'round':r['round'],'parents':r['parents'],'reads':rs[i],'rhs':ws[i]} for i,r in sorted(events.items())]
    return {'scope':'native Math trace rounds 1-6 imported into tier1; template vertices, witnessed logical constructor keys at event time, not final snapshot nodes or executable macros',
            'templates':[{k:t[k] for k in ['id','kind','rule','parents']} for t in tier1['templates']],
            'records':records,'skipped_unknown_write_schema':skipped,
            'definitions':manifest['rule_dictionary'],'source':'experiments/annotated_export/math_microbenchmark/source.egg'}


def fractions(c):return Fraction(c['rhs_num'],max(1,c['lhs_num']))


def candidates(data,max_depth=4,max_nodes=32):
    records={r['id']:r for r in data['records']}
    templates={t['id']:t for t in data['templates']}
    occurrences=defaultdict(list)
    for r in records.values():occurrences[r['template']].append(r['id'])
    earliest={t:min(occurrences[t]) if occurrences[t] else -1 for t in templates}
    for t in templates.values():
        for p in t['parents']:assert earliest[p['template']]<earliest[t['id']], 'template DAG must be acyclic'
    result=[];seen=set()
    def measure(root,members):
        stack=[root];inside=set()
        while stack:
            i=stack.pop()
            if i in inside:continue
            inside.add(i)
            stack.extend(p for p in records[i]['parents'] if records[p]['template'] in members)
        # All occurrences of an included parent role are retained, including
        # distinct instances of the same template. Do not confuse template sharing with event sharing.
        lhs={};rhs={}
        for i in sorted(inside):
            for r in records[i]['reads']:
                if r['producer'] not in inside:lhs.setdefault(key(r),r)
            for r in records[i]['rhs']:rhs.setdefault(key(r),r)
        return {'lhs_num':len(lhs),'rhs_num':len(rhs),'lhs':list(lhs.values()),'rhs':list(rhs.values()),'events':sorted(inside),'root_event':root}
    for root,t in templates.items():
        if t['kind']=='Empty':
            result.append({'id':len(result),'root':root,'members':[root],'lhs_num':0,'rhs_num':0,'lhs':[],'rhs':[],'events':[],'root_event':None,'score_range':[0,0]});continue
        ancestors=set();todo=[root]
        while todo:
            u=todo.pop()
            if u in ancestors:continue
            ancestors.add(u);todo.extend(p['template'] for p in templates[u]['parents'])
        # Longest distance produces convex suffixes: no A -> outside B -> inside C.
        distance={root:0}
        for u in sorted(ancestors,key=lambda u:earliest[u],reverse=True):
            for p in templates[u]['parents']:
                v=p['template'];distance[v]=max(distance.get(v,0),distance[u]+1)
        for depth in range(max_depth+1):
            members=frozenset(u for u in ancestors if distance[u]<=depth and templates[u]['kind']!='Empty')
            if len(members)>max_nodes or members in seen:continue
            seen.add(members)
            measured=[measure(i,members) for i in occurrences[root]]
            representative=min(measured,key=lambda m:(fractions(m),m['root_event']))
            lo=min(fractions(m) for m in measured);hi=max(fractions(m) for m in measured)
            result.append({'id':len(result),'root':root,'members':sorted(members),'score_range':[float(lo),float(hi)],'root_instances_measured':len(measured),**representative})
    return result


def partition(cs,all_nodes,edges=()):
    blocks={};owner={}
    for c in cs:
        if len(c['members'])==1:
            blocks[c['id']]=c
            for n in c['members']:owner[n]=c['id']
    assert set(owner)==set(all_nodes)
    baseline=sum((fractions(c) for c in blocks.values()),Fraction())
    heap=[];byid={c['id']:c for c in cs}
    def proposal(c):
        touched={owner[n] for n in c['members']}
        if len(touched)<=1:return None
        if set().union(*(set(blocks[b]['members']) for b in touched))!=set(c['members']):return None
        gain=fractions(c)-sum((fractions(blocks[b]) for b in touched),Fraction())
        return gain,touched
    for c in cs:
        if p:=proposal(c):heapq.heappush(heap,(-p[0],-len(c['members']),-fractions(c),c['id']))
    changes=[];cycle_rejected=0
    def quotient_acyclic(touched):
        vertices=(set(blocks)-touched)|{-1};adj={v:set() for v in vertices};degree={v:0 for v in vertices}
        for a,b in edges:
            x=-1 if owner[a] in touched else owner[a];y=-1 if owner[b] in touched else owner[b]
            if x!=y and y not in adj[x]:adj[x].add(y);degree[y]+=1
        todo=[v for v in vertices if degree[v]==0];count=0
        while todo:
            v=todo.pop();count+=1
            for w in adj[v]:
                degree[w]-=1
                if degree[w]==0:todo.append(w)
        return count==len(vertices)
    while heap:
        old_gain,_,_,i=heapq.heappop(heap);c=byid[i];p=proposal(c)
        if p is None:continue
        gain,touched=p
        if gain<0:continue
        if gain != -old_gain:
            heapq.heappush(heap,(-gain,-len(c['members']),-fractions(c),i));continue
        if not quotient_acyclic(touched):cycle_rejected+=1;continue
        for b in touched:del blocks[b]
        blocks[i]=c
        for n in c['members']:owner[n]=i
        changes.append({'block':i,'gain':float(gain),'replaced_blocks':len(touched)})
    final=sum((fractions(c) for c in blocks.values()),Fraction())
    flat=[n for c in blocks.values() for n in c['members']]
    assert len(flat)==len(set(flat))==len(all_nodes) and set(flat)==set(all_nodes)
    assert final>=baseline
    assert quotient_acyclic(set())
    return list(blocks.values()),{'singleton_objective':float(baseline),'partition_objective':float(final),'covered_nodes':len(flat),'blocks':len(blocks),'merges':changes,'quotient_acyclic_verified':True,'cycle_rejected_merges':cycle_rejected,'disjoint_cover_verified':True,'optimality':'greedy nonnegative-gain merges of convex ancestor windows; not a global optimum'}


def display(c):
    names={};literals={}
    def value(scope,sort,v):
        identity=(scope,sort,v)
        if sort!='Math':
            if identity not in literals:literals[identity]='k'+str(len(literals))
            return literals[identity]
        if identity not in names:names[identity]='v'+str(len(names))
        return names[identity]
    def row(r):
        args=[value(r['scope'],s,v) for s,v in zip(r['sorts'][:-1],r['values'][:-1])]
        out=value(r['scope'],r['sorts'][-1],r['values'][-1])
        return f'{out} = {r["op"]}('+', '.join(args)+')'
    lhs=[row(r) for r in c['lhs']];rhs=[row(r) for r in c['rhs']]
    return {'lhs':lhs,'rhs':rhs,'opaque_literals':{name:{'sort':k[1],'value':k[2]} for k,name in literals.items()}}


def run(data,out):
    cs=candidates(data);selected,stats=partition(cs,[t['id'] for t in data['templates']],[(p['template'],t['id']) for t in data['templates'] for p in t['parents']]);chosen={c['id'] for c in selected}
    ordered=sorted(cs,key=lambda c:(-fractions(c),-c['rhs_num'],c['lhs_num'],c['id']))
    ranked=[]
    for rank,c in enumerate(ordered,1):
        r={k:v for k,v in c.items() if k not in ['lhs','rhs']}
        r.update(rank=rank,score=float(fractions(c)),selected=c['id'] in chosen,zero_lhs=c['lhs_num']==0)
        if rank<=100 or c['id'] in chosen:r.update(display(c))
        ranked.append(r)
    report={'scope':data['scope'],'score':'rhs_num / max(1,lhs_num); Empty scores zero; RHS includes deduplicated existing outputs, not only new Inserted rows','candidate_limit':{'ancestor_depth':4,'template_nodes':32},'candidate_count':len(cs),'template_nodes':len(data['templates']),'instance_events':len(data['records']),'partition':stats,'rankings':ranked}
    out.mkdir(parents=True,exist_ok=True);(out/'ranking.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    event_names={x['id']:x['rule'] for x in data['records']}
    def card(r):
        rules=' / '.join(event_names[i] for i in r['events'])
        title=f"#{r['rank']}　分数 {r['score']:.3f}　LHS {r['lhs_num']} → RHS {r['rhs_num']}　{len(r['members'])} 个模板节点"
        used=list(dict.fromkeys(event_names[i] for i in r['events']))
        definitions='\n\n'.join(data['definitions'][name]['definition'].replace('\\n','\n') for name in used)
        code='<h3>引用的 tier-0 原规则（去重，非融合宏）</h3><pre>'+html.escape(definitions or 'Empty')+'</pre>'
        return '<details><summary>'+html.escape(title)+'</summary><p>'+html.escape(rules or 'Empty')+'</p><div class="pair"><pre>'+html.escape('\n'.join(r.get('lhs',[])) or '∅')+'</pre><b>→</b><pre>'+html.escape('\n'.join(r.get('rhs',[])) or '∅')+'</pre></div><p>见证事件：'+html.escape(str(r['events']))+'；k 为保留类型的原始值引用，不是解码后的数值。</p>'+code+'</details>'
    page='''<!doctype html><meta charset="utf-8"><title>Combined rule 排序</title><style>body{font:16px system-ui;margin:28px;background:#fafaf7;color:#263637}details{padding:12px;border-bottom:1px solid #ccd5d3}summary{cursor:pointer}.pair{display:grid;grid-template-columns:1fr 40px 2fr;gap:12px}pre{white-space:pre-wrap;background:#edf2f5;padding:12px}p{line-height:1.6}</style><h1>Combined rule 排序与分块</h1>'''
    page+='<p><a href="ranked.egg">当前 tier-0 原规则 .egg</a> · <a href="../tier1_extract/index.html">tier-1 / tier-0 逐节点对照</a></p>'
    page+=f'<p>真实 Math 前 6 轮：{len(data["templates"])} 个 tier-1 组合模板节点，{len(data["records"])} 个应用实例。元数据节点不参与分块，Empty 单独覆盖。</p>'
    page+=f'<p>分数 = RHS / max(1,LHS)。单节点基线 {stats["singleton_objective"]:.3f} → 贪心分块 {stats["partition_objective"]:.3f}；共 {stats["blocks"]} 块，不重叠覆盖及块间无环已验证。不是全局最优。</p>'
    page+='<p>计数基于历史见证中的去重构造器键；LHS 是块外支撑，RHS 包含中间产出。这不是最终快照覆盖率，也不是已经可执行的通用快捷规则。</p><h2>已选分块</h2>'
    page+=''.join(card(r) for r in ranked if r['selected'])+'<h2>最高优先级候选（前 100）</h2>'+''.join(card(r) for r in ranked[:100])
    (out/'index.html').write_text(page)
    return report

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--profile');p.add_argument('--manifest');p.add_argument('--tier1');p.add_argument('--input',default=str(Path(__file__).with_name('input.json')));p.add_argument('--output',default=str(Path(__file__).parent));a=p.parse_args()
    if a.profile:
        paths=[a.profile,a.manifest,a.tier1];data=capture(*(json.loads(Path(x).read_text()) for x in paths));data['input_sha256']={Path(x).name:hashlib.sha256(Path(x).read_bytes()).hexdigest() for x in paths};Path(a.input).write_text(json.dumps(data,separators=(',',':'))+'\n')
    else:data=json.loads(Path(a.input).read_text())
    r=run(data,Path(a.output));print(json.dumps({k:r[k] for k in ['candidate_count','template_nodes','instance_events','partition']}))

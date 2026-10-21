"""Deterministic Cell work ledger. Read measurements; never invent marginal credit."""
from __future__ import annotations
import csv
import hashlib
import json
from pathlib import Path

COLUMNS = ['kind','id','label','owners','plans','consumers','requests','hits','hit_rate',
           'port_requests','port_hits','port_hit_rate','new_reads','cached_reads','cache_hit_rate',
           'watch_checks','skipped_reads','candidates_before','candidates_after','candidates_saved',
           'cpu_candidate_rows','net_seconds','status']

def read(path):
    path=Path(path)
    if path.is_dir(): path=path/('report.json' if (path/'report.json').is_file() else 'audit.json')
    return path,json.loads(path.read_text())

def ratio(a,b):
    return a/b if a is not None and b else None

def key(q): return q['ruleset'],q['rule']
def row(kind,id,**kw): return dict.fromkeys(COLUMNS) | dict(kind=kind,id=id,**kw)
def gpu(report): return report.get('gpu') or report.get('runtime') or {}
def verified(r):
    return (r.get('same_commit_verification') or {}).get('exact_rows_values_equivalence_visibility') is True or r.get('complete') is True and 'runtime' in r

def baseline_check(a,b,ap,bp):
    ga,gb=gpu(a),gpu(b)
    if not verified(a) or not verified(b):raise ValueError('candidate savings need verified graphs/frontiers')
    if not ga.get('cells',{}).get('profile') or not gb.get('cells',{}).get('profile'):raise ValueError('candidate savings need two diagnostic runs')
    if ga.get('work_profile_version')!=1 or gb.get('work_profile_version')!=1:raise ValueError('candidate savings need unified search-work telemetry v1')
    wa=json.loads((ap.parent/'workflow.json').read_text());wb=json.loads((bp.parent/'workflow.json').read_text())
    for k in ['snapshot_sha256','binary_sha256','worker_sha256','policy']:
        if wa.get(k)!=wb.get(k):raise ValueError('candidate control differs: '+k)
    ia,ib=wa.get('inference_identity',{}),wb.get('inference_identity',{})
    if not ia or not ib or ia.get('analysis')!=ib.get('analysis'):raise ValueError('candidate control needs the same frozen library')
    ca,cb=ga['cells'],gb['cells']
    if not ca.get('library') or ca.get('plan_only') or not cb.get('plan_only'):raise ValueError('expected same-library projection on / plan-only pair')
    if a.get('schedule')!=b.get('schedule') or a.get('assisted_rulesets')!=b.get('assisted_rulesets'):raise ValueError('candidate schedule differs')
    for k in ['query_routing','cache_only','compact_binding_workspace','refutation','refutation_signatures','refutation_domains','binding_connections','binding_ranges','plans']:
        if ca.get(k)!=cb.get(k):raise ValueError('candidate query configuration differs: '+k)
    if ga.get('gpu_optimizations')!=gb.get('gpu_optimizations'):raise ValueError('generic GPU optimizations differ')
    da,db=ga.get('saturation_diagnostics') or {},gb.get('saturation_diagnostics') or {}
    if bool(da)!=bool(db) or da.get('joint',{}).get('proofs_enabled')!=db.get('joint',{}).get('proofs_enabled'):raise ValueError('common proof provider differs')
    qa={key(q):q for q in ga['rules']};qb={key(q):q for q in gb['rules']}
    if qa.keys()!=qb.keys():raise ValueError('rule universe differs')
    for k in qa:
        for f in ['emitted_bindings','cell_bindings','cell_queries']:
            if qa[k]['stats'].get(f)!=qb[k]['stats'].get(f):raise ValueError('control changes query outputs/provider')
    return qb

def provenance(path):
    p=Path(path).parent/'workflow.json'
    if not p.exists():return None
    w=json.loads(p.read_text())
    return {k:w.get(k) for k in ['source_head','snapshot_sha256','binary_sha256','worker_sha256','policy','inference_identity']}

def build(path, baseline=None):
    source,r=read(path);g=gpu(r);c=g.get('cells') or {};d=g.get('saturation_diagnostics') or {}
    measured=c.get('profile') is True and g.get('work_profile_version')==1
    qs=g.get('rules',[]);plans=c.get('plans',[]);rows=[];raw={};before={}
    if baseline:
        bp,b=read(baseline);before=baseline_check(r,b,source,bp)
    fields=g.get('cell_plan_fields',[])
    work=g.get('cell_plan_work',{})
    plan_rows={}; consumers={}
    for p in plans:
        pid=p['plan'];owners=p.get('states',[]);users=p.get('consumers',[])
        values=work.get(str(pid),work.get(pid,[0]*len(fields))) if measured else []
        m=dict(zip(fields,values));raw[str(pid)]=m if measured else None
        get=lambda k:m.get(k) if measured else None
        reads,cache=get('probes'),get('row_reuses')
        pr=row('shared_projection',str(pid),label=' + '.join(str(x) for x in p.get('atoms',[])),
               owners=owners,plans=[pid],consumers=users,requests=get('segment_checks'),hits=get('segment_hits'),
               hit_rate=ratio(get('segment_hits'),get('segment_checks')),
               port_requests=get('port_queries'),port_hits=get('port_nonempty_queries'),
               port_hit_rate=ratio(get('port_nonempty_queries'),get('port_queries')),
               new_reads=reads,cached_reads=cache,cache_hit_rate=ratio(cache,cache+reads if cache is not None and reads is not None else None),
               watch_checks=get('watch_checks'),skipped_reads=get('skipped_internal_reads'),
               status='measured shared work; seconds not isolated' if measured else 'not measured: collect diagnostic profile')
        rows.append(pr);plan_rows[pid]=pr
        for qid in users:consumers.setdefault(qid,set()).add(pid)
    library=c.get('library') or {}
    for cid in library.get('loaded_states',[]):
        related=[p['plan'] for p in plans if cid in p.get('states',[])]
        rows.append(row('analyzed_cell',str(cid),label=f'Analyze Cell {cid}',plans=related,
            consumers=sorted({q for p in related for q in plan_rows[p]['consumers']}),
            status='shared projections: follow plan rows; do not duplicate credit' if related else 'no compiled projection; no runtime reuse'))
    for rejected in library.get('rejected',[]):
        rows.append(row('analyzed_cell',str(rejected['state']),label=f"Analyze Cell {rejected['state']}",plans=[],consumers=[],status='rejected: '+rejected['reason']))
    # Source programs are a separate namespace; no promotion to learned Cells.
    by_rule={key(q):q for q in qs}
    for p in d.get('programs',[]):
        q=by_rule.get((p['ruleset'],p['rule']),{});st=q.get('stats',{})
        fresh,cache=p.get('index_reads'),p.get('reused_port_reads')
        rows.append(row('source_program',str(p['program_id']),label=p['seed'],
            cpu_candidate_rows=p.get('candidate_rows'),
            requests=st.get('cell_queries'),hits=st.get('cell_nonempty_queries'),
            hit_rate=ratio(st.get('cell_nonempty_queries'),st.get('cell_queries')),
            new_reads=fresh,cached_reads=cache,cache_hit_rate=ratio(cache,cache+fresh if cache is not None and fresh is not None else None),
            status='source-compiled execution; not independent Cell proof credit'))
    if d.get('joint'):
        j=d['joint'];stats=j.get('stats',{});comp=j.get('comparison_closure',{})
        rows.append(row('proof_group','terms-joint-closure-proofs',label='joint closure proofs',
            requests=stats.get('boundaries'),hits=stats.get('skipped_joint_saturations'),
            hit_rate=ratio(stats.get('skipped_joint_saturations'),stats.get('boundaries')),
            status=f"comparison closures consumed: {comp.get('saturations','unmeasured')}; marginal time needs cell-credit"))
    for qid,pids in sorted(consumers.items()):
        q=qs[qid];st=q['stats'];values=st.get('search_work') if measured else None
        m=dict(zip(g.get('search_work_fields',[]),values or []));old=before.get(key(q),{}).get('stats',{}).get('search_work')
        bm=dict(zip(g.get('search_work_fields',[]),old or []));a=m.get('candidate_rows');b=bm.get('candidate_rows')
        rows.append(row('consumer',str(qid),label=q['ruleset'],plans=sorted(pids),consumers=[qid],
            requests=st.get('dispatches'),hits=st.get('emitted_bindings'),
            candidates_before=b,candidates_after=a,candidates_saved=b-a if a is not None and b is not None else None,
            status=f"generic empty requests: {st.get('empty_table_prunes',0)}; hits column = output bindings, not a rate"))
    # Connected components of plan <-> consumer edges own each query delta once.
    remaining=set(plan_rows);groups=[]
    while remaining:
        component={min(remaining)};users=set()
        while True:
            new_users={q for p in component for q in plan_rows[p]['consumers']}
            new_plans=component|{p for q in new_users for p in consumers[q]}
            if new_plans==component:users=new_users;break
            component=new_plans
        remaining-=component
        cr=[v for v in rows if v['kind']=='consumer' and int(v['id']) in users]
        savings=[v['candidates_saved'] for v in cr]
        groups.append(dict(id=len(groups),plans=sorted(component),consumers=sorted(users),
            owners=sorted({x for p in component for x in plan_rows[p]['owners']}),
            candidates_saved=sum(savings) if cr and all(v is not None for v in savings) else None,
            net_seconds=None,scope='query work difference for the connected sharing group; no per-parent credit'))
    return dict(schema='cell-work-ledger/v1',source=str(source),source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        baseline=str(baseline) if baseline else None,baseline_sha256=hashlib.sha256(bp.read_bytes()).hexdigest() if baseline else None,
        provenance=provenance(source),baseline_provenance=provenance(bp) if baseline else None,
        verified=verified(r),profile_measured=measured,
        definitions={'segment_hit_rate':'successful certified segment substitutions / segment requests',
            'port_hit_rate':'port evaluations with at least one emitted binding / port evaluations',
            'program_hit_rate':'nonempty complete query answers / query requests',
            'candidate_rows':'live row visits before binding constraints, including retry attempts; not unique rows, matches or mutations',
            'credit':'per-object seconds remain null without an isolated valid timing ablation; shared groups must not be summed again through parent Cells'},
        rows=rows,groups=groups,plan_counters=raw,plan_definitions=plans,source_program_counters=d.get('programs',[]),proof_group_counters=d.get('joint'),
        runtime_seconds=g.get('runtime',{}).get('run_seconds'),
        timing_eligible=not c.get('profile') and not g.get('audit') and not g.get('query_timing_profile') and verified(r),
        shared_costs={'commit_marker_cpu_work':g.get('commit_markers'),'bake_seconds':None,'allocated_gpu_bytes':g.get('device',{}).get('device_bytes'),
            'cell_cache_bytes':g.get('device',{}).get('cell_cache_bytes'),
            'cell_watch_bytes':g.get('device',{}).get('cell_watch_bytes'),
            'watch_checks_not_assigned_to_valid_plan':g.get('cell_work',{}).get('watch_entries_checked',0)-sum((m or {}).get('watch_checks',0) for m in raw.values()) if measured else None},
        limitations=['No sampled or cache-hit-derived wall-time credit.', 'Unmeasured counters are null, never zero.',
                     'Shared fragment work is recorded once; ambiguous parent Cell attribution stays unresolved.'])

def display(v):
    if v is None:return '—'
    if isinstance(v,list):return ','.join(map(str,v))
    if isinstance(v,float):return f'{v:.4f}'
    return str(v).replace('|','\\|').replace('\n',' ')

def write(path, baseline=None, output=None, timing=None):
    result=build(path,baseline);source=Path(result['source']);stem=Path(output) if output else source.parent/'cell-ledger'
    if timing:
        tp,tr=read(timing);tg=gpu(tr)
        if not verified(tr) or tg.get('cells',{}).get('profile') or tg.get('audit'):raise ValueError('invalid uninstrumented ledger timing')
        tw=json.loads((tp.parent/'workflow.json').read_text());sw=json.loads((source.parent/'workflow.json').read_text())
        for f in ['snapshot_sha256','binary_sha256','worker_sha256','policy']:
            if tw.get(f)!=sw.get(f):raise ValueError('ledger timing differs: '+f)
        if tw.get('inference_identity',{}).get('analysis')!=sw.get('inference_identity',{}).get('analysis'):raise ValueError('ledger timing library differs')
        result['timing']={'source':str(tp),'inference_seconds':tg['runtime']['run_seconds'],'setup_seconds':tg['runtime'].get('setup_seconds'),'scope':'whole execution; not per-Cell credit'}
    stem.parent.mkdir(parents=True,exist_ok=True)
    dest=stem.with_suffix('.json')
    if dest.exists() and json.loads(dest.read_text()).get('schema')!='cell-work-ledger/v1':raise ValueError('refusing to overwrite unrelated ledger file')
    dest.write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n')
    with stem.with_suffix('.csv').open('w',newline='') as f:
        w=csv.DictWriter(f,fieldnames=COLUMNS,lineterminator="\n");w.writeheader()
        for r in result['rows']:w.writerow({k:json.dumps(v,ensure_ascii=False) if isinstance(v,list) else v for k,v in r.items()})
    lines=['# Cell 代价与复用账表','',f"来源：`{source}`。诊断计数：{result['profile_measured']}。核对通过：{result['verified']}。",'',
           '— 表示未测或不能独立归因。共享片段只记一次；candidate 是候选行访问，不是 match 或新事实。','']
    layouts={
        'analyzed_cell':['id','plans','consumers','net_seconds','status'],
        'shared_projection':['id','owners','requests','hits','hit_rate','port_requests','port_hits','port_hit_rate','new_reads','cached_reads','watch_checks','skipped_reads'],
        'source_program':['id','label','requests','hits','hit_rate','new_reads','cached_reads','cache_hit_rate','net_seconds'],
        'proof_group':['id','requests','hits','hit_rate','net_seconds','status'],
        'consumer':['id','label','plans','requests','candidates_before','candidates_after','candidates_saved','status'],
    }
    for kind in ['analyzed_cell','shared_projection','source_program','proof_group','consumer']:
        entries=[r for r in result['rows'] if r['kind']==kind]
        if not entries:continue
        columns=layouts[kind]
        titles={'analyzed_cell':'Analyze Cell','shared_projection':'共享片段','source_program':'源码编译程序','proof_group':'闭包证明组','consumer':'原始查询'}
        labels={'id':'ID','label':'对象','owners':'来源 Cell','plans':'共享片段','consumers':'查询 ID','requests':'请求','hits':'命中','hit_rate':'命中率','port_requests':'端口请求','port_hits':'端口非空回答','port_hit_rate':'端口命中率','new_reads':'新读取','cached_reads':'缓存读取','cache_hit_rate':'缓存率','watch_checks':'订阅检查','skipped_reads':'跳过读取','candidates_before':'原候选访问','candidates_after':'现候选访问','candidates_saved':'省去候选访问','net_seconds':'独立净收益秒','status':'口径'}
        lines += ['## '+titles[kind],'','| '+' | '.join(labels[k] for k in columns)+' |','|'+'|'.join(['---']*len(columns))+'|']
        lines += ['| '+' | '.join((f'{100*r[k]:.2f}%' if k.endswith('_rate') and r.get(k) is not None else display(r.get(k))) for k in columns)+' |' for r in entries];lines.append('')
    lines += ['## 共享归因组','', '```json',json.dumps(result['groups'],ensure_ascii=False,indent=2),'```','',
              '完整指标、port 命中率和口径见同名 JSON/CSV。组的候选访问差额不自动等于独立 Cell credit。','']
    stem.with_suffix('.md').write_text('\n'.join(lines))
    return result

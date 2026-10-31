"""Read-only local graph slices over actual native stage archives."""
import collections
import hashlib
import json
import os
from pathlib import Path
import random
import signal
import shutil
import subprocess
import sys
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
RUNS = ROOT / 'out/stage-sampler'
JOBS = {}
LOCK = threading.RLock()
CACHE = collections.OrderedDict()
PRESETS = [
    {'id':'math','label':'Math 微子集 · 交换 / 分配 / 因式提取','path':'tools/egglog_debugger/fixtures/stage-math.egg'},
    {'id':'merge','label':'多阶段候选选择 · Smaller / merge / union','path':'tools/egglog_debugger/fixtures/stage-merge.egg'},
    *[{'id':n,'label':f'eggcc {n} · 真实 Pass 3','suite':n} for n in ('fact','loopfact','sum-bits','raytrace')],
]


def atomic(path, data):
    temp=path.with_suffix(path.suffix+'.pending')
    temp.write_text(json.dumps(data,ensure_ascii=False));temp.replace(path)


def safe_folder(path):
    folder=(ROOT/path).resolve()
    if not folder.is_relative_to((ROOT/'out').resolve()):raise ValueError('请选择仓库 out/ 下的运行目录')
    return folder


def lines(path):
    if not path.exists():return []
    # A live writer may not have completed the last line yet.
    text=path.read_bytes();text=text[:text.rfind(b'\n')+1]
    return [json.loads(line) for line in text.splitlines() if line]


def catalog():
    saved=[]
    if RUNS.exists():
        for p in RUNS.glob('*/job.json'):
            try:
                j=json.loads(p.read_text());saved.append({'path':str(p.parent.relative_to(ROOT)),'label':j['label'],'status':j['status'],'created':j['created']})
            except (OSError,ValueError,KeyError):pass
    return {'presets':PRESETS,'runs':sorted(saved,key=lambda j:j['created'],reverse=True)}


def start(data):
    with LOCK:
        if any(j['info']['status'] in ('preparing','running') for j in JOBS.values()):raise ValueError('已有采集任务运行中；可以先停止它，或查看已记录的阶段。')
    preset=next((p for p in PRESETS if p['id']==data.get('preset')),None)
    if preset and 'suite' in preset:
        suite=ROOT/'experiments/profile_suite';spec=json.loads((suite/'suite.json').read_text())
        c=next(c for c in spec['cases'] if c['name']==preset['suite'])
        source=b''.join((suite/p).read_bytes() for p in c['parts'])
        if hashlib.sha256(source).hexdigest()!=c['sha256']:raise ValueError('冻结样例的校验和不一致')
        label=preset['label']
    else:
        file=(ROOT/(preset['path'] if preset else data.get('source_path',''))).resolve()
        if not file.is_relative_to(ROOT) or not file.is_file() or file.suffix!='.egg':raise ValueError('请选择本仓库内的 .egg 文件')
        source=file.read_bytes();label=preset['label'] if preset else file.name
    folder=RUNS/uuid.uuid4().hex;folder.mkdir(parents=True);(folder/'source.egg').write_bytes(source)
    info={'schema':'eggfuse-stage-sampler-job/v1','path':str(folder.relative_to(ROOT)),'label':label,'status':'preparing','created':time.time(),'source_sha256':hashlib.sha256(source).hexdigest(),'engine':'native CPU · staged','error':None}
    job={'folder':folder,'info':info,'cancel':False,'process':None};atomic(folder/'job.json',info)
    with LOCK:JOBS[info['path']]=job
    def work():
        def run(cmd,log):
            with LOCK:
                if job['cancel']:raise InterruptedError('用户取消')
                proc=subprocess.Popen(cmd,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,start_new_session=True);job['process']=proc
            code=proc.wait()
            with LOCK:job['process']=None
            if job['cancel']:raise InterruptedError('用户取消')
            if code:raise RuntimeError(f'执行退出码 {code}')
        try:
            with (folder/'worker.log').open('w') as log:
                run(['cargo','build','--offline','--release','--manifest-path','engine/cpu/Cargo.toml'],log)
                binary=ROOT/'engine/cpu/target/release/eggfuse-cpu'
                frozen=folder/'native-cpu'
                try:os.link(binary,frozen)
                except OSError:shutil.copy2(binary,frozen)
                info['binary_sha256']=hashlib.sha256(frozen.read_bytes()).hexdigest()
                run([str(frozen),'--source',str(folder/'source.egg'),'--prepare',str(folder/'core.json')],log)
                if str(ROOT) not in sys.path:sys.path.insert(0,str(ROOT))
                from eggfuse.compiler.targets.cpu import lower
                core=json.loads((folder/'core.json').read_text());(folder/'program.json').write_text(json.dumps(lower(core)))
                info['status']='running';atomic(folder/'job.json',info)
                run([str(frozen),'--source',str(folder/'source.egg'),'--plan',str(folder/'program.json'),'--mode','staged','--stage-archive',str(folder/'archive'),'--no-state-export','--brief-report','--output',str(folder/'runtime')],log)
            info['status']='complete'
        except InterruptedError:info['status']='cancelled'
        except Exception as e:
            info['status']='error';tail=(folder/'worker.log').read_text(errors='replace')[-8000:] if (folder/'worker.log').exists() else ''
            info['error']=str(e)+'\n'+tail
        finally:atomic(folder/'job.json',info)
    threading.Thread(target=work,daemon=True).start();return info


def cancel(path):
    with LOCK:
        job=JOBS.get(path)
        if job:
            job['cancel']=True
            if job['process'] and job['process'].poll() is None:
                try:os.killpg(job['process'].pid,signal.SIGTERM)
                except ProcessLookupError:pass
    return status(path)


def status(path):
    folder=safe_folder(path);info=json.loads((folder/'job.json').read_text())
    info['stages']=[{k:v for k,v in stage.items() if k!='matches'} for stage in lines(folder/'archive/stages.ndjson')]
    if (folder/'archive/manifest.json').exists():
        core=json.loads((folder/'archive/manifest.json').read_text())['core']
        info['sorts']=sorted({s['name'] for t in core['tables'] for s in t['schema'] if s['kind']=='eq'})
        info['rulesets']=[s['name'] for s in core['rulesets']]
    else:info.update(sorts=[],rulesets=[])
    return info


class Archive:
    def __init__(self,folder):
        self.folder=folder;self.core=json.loads((folder/'manifest.json').read_text())['core'];self.lock=threading.RLock();self.reset()
    def reset(self):
        self.values=[];self.rows={};self.aliases={};self.frame=-1;self.offset=0
    def seek(self,target):
        if target<self.frame:self.reset()
        with (self.folder/'frames.ndjson').open('rb') as f:
            f.seek(self.offset)
            while self.frame<target:
                raw=f.readline()
                if not raw.endswith(b'\n'):raise ValueError('这个阶段尚未记录完成')
                j=json.loads(raw)
                if j['frame']!=self.frame+1 or j['value_start']!=len(self.values):raise ValueError('阶段增量链不连续')
                self.values.extend(j['values'])
                for c in j['changes']:
                    t=c['table']
                    for k in c['removed']:self.rows.pop((t,tuple(k)),None)
                    for row in c['upserts']:self.rows[(t,tuple(row[:-2]))]=(t,tuple(row[:-1]),bool(row[-1]))
                self.aliases.update(j['aliases']);self.frame=j['frame'];self.offset=f.tell()
        return {'rows':dict(self.rows),'aliases':dict(self.aliases),'values':self.values,'frame':self.frame}
    def pair(self,stage):
        with self.lock:
            before=self.seek(stage['before']);after=self.seek(stage['after'])
            return before,after


def archive(path):
    folder=safe_folder(path)/'archive'
    with LOCK:
        key=str(folder)
        if key not in CACHE:CACHE[key]=Archive(folder)
        CACHE.move_to_end(key)
        while len(CACHE)>2:CACHE.popitem(last=False)
        return CACHE[key]


def canonical(i,aliases):
    seen=set()
    while i in aliases and aliases[i]!=i:
        if i in seen:raise ValueError('无效的 union 链')
        seen.add(i);i=aliases[i]
    return i


def row_id(row):
    return 'r'+hashlib.sha256(json.dumps(row[:2],separators=(',',':')).encode()).hexdigest()[:20]


def q(text):return json.dumps(str(text),ensure_ascii=False)


def build_sample(core,before,after,stage,request):
    side=request.get('side','after')
    if side not in ('before','after'):raise ValueError('请选择阶段入口或出口')
    snap=before if side=='before' else after;values=snap['values'];tables=core['tables']
    depth=max(0,min(8,int(request.get('depth',2))));budget=max(1,min(160,int(request.get('budget',30))))
    alternatives=max(1,min(20,int(request.get('alternatives',3))));seed=int(request.get('seed',0));rng=random.Random(seed)
    hidden=bool(request.get('hidden',False));context=bool(request.get('context',True));focus=request.get('focus','reads')
    members=collections.defaultdict(list);incoming=collections.defaultdict(list);eqs=set()
    visible=[r for r in snap['rows'].values() if hidden or not r[2]]
    for row in visible:
        t,vs,_=row
        for v in vs:
            if 'eq' in values[v]:eqs.add(v)
        if 'eq' in values[vs[-1]]:members[vs[-1]].append(row)
        for v in set(vs[:-1]):
            if 'eq' in values[v]:incoming[v].append(row)
    ruleset=next((s for s in core['rulesets'] if s['name']==stage['ruleset']),{'rules':[]})
    reads={a['call']['table'] for r in ruleset['rules'] for a in r['body'] if 'table' in a['call']}
    read_ids={i for i,t in enumerate(tables) if t['name'] in reads}
    counts={m['rule']:m['count'] for m in stage.get('matches',[])}
    rules=[]
    duplicate_names=collections.Counter(r['name'] for r in ruleset['rules'])
    for r in ruleset['rules']:
        rules.append({'name':r['name'],'code':rule_code(r),'matches':counts.get(r['name'],0) if duplicate_names[r['name']]==1 else None,'group_matches':counts.get(r['name'],0),'reads':sorted({a['call']['table'] for a in r['body'] if 'table' in a['call']}),'writes':sorted({a['call']['table'] for a in r['head'] if 'call' in a and 'table' in a['call']}),'guards':sorted({a['call']['primitive'] for a in r['body'] if 'primitive' in a['call']})})
    rules.sort(key=lambda r:(-(r['matches'] or 0),r['name']))
    after_alias=after['aliases']
    old={}
    for t,vs,h in before['rows'].values():
        mapped=tuple(canonical(v,after_alias) for v in vs)
        old[(t,mapped[:-1])]=(mapped,h)
    after_rows={(t,vs[:-1]):(vs,h) for t,vs,h in after['rows'].values()}
    def change(row):
        t,vs,h=row
        if side=='before':
            mapped=tuple(canonical(v,after_alias) for v in vs)
            later=after_rows.get((t,mapped[:-1]))
            return 'removed' if later is None else 'updated' if later!=(mapped,h) else 'existing'
        prior=old.get((t,vs[:-1]))
        return 'added' if prior is None else 'updated' if prior!=(vs,h) else 'existing'
    before_eqs={v for _,vs,_ in before['rows'].values() for v in vs if 'eq' in values[v]}
    merged=collections.defaultdict(list)
    for v in before_eqs:
        target=canonical(v,after_alias)
        if target!=v:merged[target].append(v)
    changed=set()
    for row in visible:
        if change(row)!='existing':changed.update(v for v in row[1] if 'eq' in values[v])
    changed.update(merged if side=='after' else (v for group in merged.values() for v in group))
    related={v for r in visible if r[0] in read_ids for v in r[1] if 'eq' in values[v]}
    choices=eqs & (related if focus=='reads' and stage['ruleset'] else changed if focus=='changed' else eqs)
    root=request.get('root');root=int(root) if root is not None else None
    if root is not None and side=='after':root=canonical(root,after_alias)
    counts=collections.Counter(values[v]['sort'] for v in choices)
    auto_sort=values[root]['sort'] if root is not None and 0<=root<len(values) else 'Expr' if 'Expr' in counts else 'Math' if 'Math' in counts else max(counts,key=lambda s:(not s.startswith('_'),counts[s],s),default='')
    sort=request.get('sort') or auto_sort
    choices=sorted(v for v in choices if not sort or values[v]['sort']==sort)
    if root is not None and side=='after':root=canonical(root,after_alias)
    if root is not None and (root not in eqs or (sort and values[root]['sort']!=sort)):
        return {'empty':True,'message':'固定的根在这一时刻不存在。它可能尚未生成、已删除，或需要切换到阶段出口。','available_roots':len(choices),'stage':stage,'side':side,'seed':seed,'rules':rules}
    if root is None and choices:root=rng.choice(choices)
    if root is None:
        return {'empty':True,'message':'该筛选下没有可采样的位置。可切换为「全图」或其他 sort。','available_roots':len(choices),'stage':stage,'side':side,'seed':seed,'rules':rules}
    forced={int(x) for x in request.get('expanded',[])}
    blocked={int(x) for x in request.get('collapsed',[])}
    if len(forced)>160 or len(blocked)>160:raise ValueError('展开位置过多，请重置切片')
    if side=='after':
        forced={canonical(x,after_alias) for x in forced};blocked={canonical(x,after_alias) for x in blocked}
    blocked.discard(root)
    expanded=set();selected={};frontier=set();more={};queue=[(root,0)];visited=set()
    while queue and len(selected)<budget:
        queue.sort(key=lambda x:(0 if x[0] in forced else 1,x[1],x[0]));v,d=queue.pop(0)
        if v in visited:continue
        visited.add(v)
        if v in blocked or (d>=depth and v!=root and v not in forced):frontier.add(v);continue
        rows=sorted(members[v],key=lambda r:(0 if r[0] in read_ids else 1,row_id(r)))
        # Show a bounded number of equivalent enodes, never pick one as the class.
        limits=request.get('class_limits',{})
        limit=max(alternatives,min(budget,int(limits.get(str(v),alternatives*(2 if v in forced else 1)))))
        kept=rows[:limit];more[v]=max(0,len(rows)-len(kept))
        if context and v==root:
            extra=sorted(incoming[v],key=lambda r:(0 if r[0] in read_ids else 1,row_id(r)))
            kept+=extra[:alternatives]
        if not kept:frontier.add(v);continue
        expanded.add(v)
        for row in kept:
            if row[1][-1] in blocked:continue
            rid=row_id(row)
            if len(selected)>=budget:break
            selected[rid]=row
            for child in row[1]:
                if 'eq' in values[child] and child!=v:queue.append((child,d+1))
    used_eq={v for r in selected.values() for v in r[1] if 'eq' in values[v]}|{root}
    expanded={r[1][-1] for r in selected.values() if 'eq' in values[r[1][-1]]}
    more={v:max(0,len(members[v])-sum(r[1][-1]==v for r in selected.values())) for v in expanded}
    frontier=used_eq-expanded
    colors={'existing':('#f8fafc','#94a3b8'),'added':('#dcfce7','#16a34a'),'updated':('#f3e8ff','#9333ea'),'removed':('#fee2e2','#dc2626')}
    dot=['digraph sample {','graph [rankdir=TB, compound=true, bgcolor="transparent", pad="0.20", nodesep="0.16", ranksep="0.45", fontname="Helvetica"];','node [fontname="Helvetica", fontsize=11]; edge [fontname="Helvetica", fontsize=9, color="#64748b"];']
    items={};ports=[]
    def value_text(v):
        x=values[v]
        if 'eq' in x:return x['eq']
        if 'elements' in x:return f"{x['sort']} · {len(x['elements'])} 项"
        if x['sort']=='Unit':return '()'
        if x['sort']=='bool':return 'true' if str(x.get('value'))=='1' else 'false'
        if x['sort']=='String':return json.dumps(x.get('value'),ensure_ascii=False)
        return str(x.get('display',x.get('value',x.get('opaque','?'))))
    for v in sorted(used_eq):
        x=values[v];port=v in frontier;label=(f"RipenInput[{v}]\n{x['sort']}" if port else f"{x['sort']} · e{v}")
        border='#d97706' if port else '#0891b2' if v in merged and side=='after' else '#2563eb';fill='#fffbeb' if port else '#ecfeff' if v in merged and side=='after' else '#eff6ff'
        if port:
            dot.append(f'c{v} [id="c{v}", URL="#class-{v}", label={q(label)}, shape=box, style="rounded,filled", fillcolor="{fill}", color="{border}"];')
        else:
            # Enclose equivalent enodes directly, rather than adding a separate
            # eclass rank and membership edges. The invisible anchor routes
            # incoming argument edges to the class boundary, not a chosen enode.
            grouped=[rid for rid,r in selected.items() if r[1][-1]==v]
            dot.append(f'subgraph cluster_c{v} {{ graph [id="c{v}", URL="#class-{v}", label={q(label)}, fontsize=11, style="rounded,filled", fillcolor="{fill}", color="{border}", penwidth={2.5 if v==root else 1.2}, margin=10];')
            dot.append(f'c{v} [shape=point, width=0, height=0, style=invis, label=""];')
            if more.get(v):
                dot.append(f'm{v} [id="m{v}",URL="#class-{v}",label={q("+"+str(more[v])+" 个等价节点")},shape=plaintext,fontcolor="#2563eb"];')
                grouped.append(f'm{v}')
            dot.append('{rank=same; '+f'c{v}; '+'; '.join(grouped)+';} }')
        items[f'c{v}']={'kind':'port' if port else 'class','value_id':v,'sort':x['sort'],'identity':x['eq'],'members':len(members[v]),'hidden_alternatives':more.get(v,0),'root':v==root,'merged_from':merged.get(v,[]) if side=='after' else []}
        if port:ports.append(items[f'c{v}'])
    by_identity={(v.get('sort'),v.get('eq')):i for i,v in enumerate(values) if 'eq' in v}
    def field(v):
        x=dict(values[v])
        if isinstance(x.get('value'),str) and len(x['value'])>1024:
            x['value_length']=len(x['value']);x['value']=x['value'][:1024]+'…';x['value_truncated']=True
        if 'elements' in x:
            x['elements_total']=len(x['elements'])
            x['elements']=[{**e,'value_id':by_identity.get((e.get('sort'),e.get('eq')))} for e in x['elements'][:16]]
        return x
    for rid,row in selected.items():
        t,vs,h=row;table=tables[t];ch=change(row);fill,border=colors[ch];literals=[]
        for i,v in enumerate(vs):
            if 'eq' not in values[v]:literals.append(f'{"out" if i==len(vs)-1 else "arg"+str(i)}: {value_text(v)}')
        label=table['name']+(' [subsumed]' if h else '')+(' = '+value_text(vs[-1]) if 'eq' not in values[vs[-1]] else '')
        if literals:label+='\n'+'\n'.join(s[:100] for s in literals[:8])
        dot.append(f'{rid} [id="{rid}",URL="#{rid}",label={q(label)},shape=box,style="rounded,filled{",dashed" if h else ""}",fillcolor="{fill}",color="{border}"];')
        for i,v in enumerate(vs[:-1]):
            if v in used_eq:
                cluster=f', lhead="cluster_c{v}"' if v in expanded and v!=vs[-1] else ''
                dot.append(f'{rid} -> c{v} [label="{i}"{cluster}];')
        items[rid]={'kind':'row','table':table['name'],'table_kind':table['kind'],'change':ch,'subsumed':h,'fields':[{'position':i,'role':'output' if i==len(vs)-1 else 'input',**field(v),'value_id':v} for i,v in enumerate(vs)]}
    dot.append('}')
    # Only this bounded graph is rendered/sent to the browser.
    source='\n'.join(dot);render=subprocess.run(['dot','-Tsvg'],input=source,capture_output=True,text=True,timeout=20)
    if render.returncode:raise ValueError('局部 DOT 渲染失败：'+render.stderr[-1000:])
    return {'empty':False,'stage':stage,'side':side,'seed':seed,'sort':values[root]['sort'],'root':root,'available_roots':len(choices),'svg':render.stdout,'dot':source,'items':items,'ports':ports,'rules':rules,'stats':{'visible_facts':len(selected),'visible_eclasses':len(expanded),'ripen_inputs':len(frontier),'snapshot_facts':len(snap['rows']),'stage_before_facts':len(before['rows']),'stage_after_facts':len(after['rows']),'changed_in_view':sum(change(r)!='existing' for r in selected.values()),'truncated':bool(frontier or any(more.values()))},'guarantee':'真实 stage 状态的局部切片；RipenInput 只折叠显示，不修改 egraph，也不构成饱和闭包证书。'}


def rule_code(rule):
    if rule['name'].lstrip().startswith('('):return rule['name']
    def val(v):
        if 'var' in v:return v['var']
        name=v['sort']['name'];x=v['literal']
        return '()' if name=='Unit' else json.dumps(x,ensure_ascii=False) if name=='String' else str(x)
    def call(c,args):return '('+c.get('table',c.get('primitive','?'))+(' '+' '.join(val(v) for v in args) if args else '')+')'
    body=[];head=[]
    for a in rule['body']:
        out=a['args'][-1];expr=call(a['call'],a['args'][:-1])
        body.append(expr if out['sort']['name']=='Unit' and 'literal' in out else '(= '+val(out)+' '+expr+')')
    for a in rule['head']:
        k=a['op']
        if k=='copy':head.append('(let '+a['dst']+' '+val(a['arg'])+')')
        elif k=='let':head.append('(let '+a['dst']+' '+call(a['call'],a['args'])+')')
        elif k=='set':head.append('(set '+call(a['call'],a['args'])+' '+val(a['value'])+')')
        elif k=='union':head.append('(union '+' '.join(val(v) for v in a['args'])+')')
        elif k in ('delete','subsume'):head.append('('+k+' '+call(a['call'],a['args'])+')')
        else:head.append('('+k+')')
    return '; '+rule['name']+' — 归一化规则，只读\n(rule (\n  '+'\n  '.join(body)+'\n) (\n  '+'\n  '.join(head)+'\n))'


def sample(data):
    path=data['path'];folder=safe_folder(path);stages=lines(folder/'archive/stages.ndjson')
    ordinal=int(data.get('stage',0));stage=next((s for s in stages if s['ordinal']==ordinal),None)
    if stage is None:raise ValueError('这个 stage 尚未记录完成')
    reader=archive(path);before,after=reader.pair(stage)
    return build_sample(reader.core,before,after,stage,data)

"""Show shared templates separately from concrete occurrences."""
from pathlib import Path
import json
import subprocess
import textwrap
OUT=Path(__file__).resolve().parent
r=json.loads((OUT/'results.json').read_text())
def q(s):return json.dumps(str(s),ensure_ascii=False)
def render(stem,lines):
    (OUT/(stem+'.dot')).write_text('\n'.join(lines)+'\n')
    subprocess.run(['dot','-Tsvg',str(OUT/(stem+'.dot')),'-o',str(OUT/(stem+'.svg'))],check=True,timeout=60)
lines=['digraph G {','graph [rankdir=LR,bgcolor="#fafaf7"];node [shape=box,style="rounded,filled",fontname="Arial"];edge [fontname="Arial"];',
       'subgraph cluster_templates {label="Tier-1 shared rule-comb templates";color="#7392b3";']
for t in r['templates']:
    label=t['kind']+' '+t['rule']
    if t['kind']!='Empty':label+='\n'+textwrap.fill(t['relative_binding'],70)
    lines.append(f'{q(t["id"])} [label={q(label)},fillcolor="#ddeafa"];')
    for p in t['parents']:lines.append(f'{q(p["template"])} -> {q(t["id"])} [label={q("parent "+str(p["slot"]))}];')
lines+=['}','subgraph cluster_instances {label="Concrete occurrences / evidence (not template children)";color="#7aa487";']
for i in r['instances']:
    label=f'Occurrence {i["event"]}'
    for b in i['bindings']:label+='\n'+b.replace('(ACons ','').replace('(ANil)','').replace(')','').replace('(V "Math" ','').replace('"','')
    label+='\n'+str(len(i['effects']))+' scoped effects'
    lines.append(f'{q(i["id"])} [label={q(label)},fillcolor="#e1f0e5"];')
lines.append('}')
for i in r['instances']:lines.append(f'{q(i["id"])} -> {q(i["template"])} [style=dashed,label="instance of"];')
render('shared_templates',lines+['}'])
nodes=r['native_egraph']['nodes'];lines=['digraph G {','graph [rankdir=LR];node [shape=box,fontsize=9];edge [fontsize=8];']
for k,n in nodes.items():
    label=n['op'][:70]+'\n'+n['eclass'];lines.append(f'{q(k)} [label={q(label)},tooltip={q(n["op"])}];')
    for j,c in enumerate(n['children']):assert c in nodes;lines.append(f'{q(k)} -> {q(c)} [label="{j}"];')
render('shared_templates.native',lines+['}'])
(OUT/'shared_templates.native.json').write_text(json.dumps(r['native_egraph'],ensure_ascii=False,indent=2)+'\n')
(OUT/'index.html').write_text('''<!doctype html><meta charset="utf-8"><title>Tier-1 shared rule-comb templates</title>
<style>body{font:16px system-ui;margin:24px;background:#fafaf7;color:#263637}iframe{width:100%;height:80vh;border:1px solid #bbb}a{margin-right:24px}</style>
<h1>Tier-1：共享 rule-comb 模板</h1><p><a href="../comb_order/index.html">真实 Math：combined rule 排序与分块</a></p><p>蓝色框只有规则、父组合与相对端口；没有 Entry 或具体数据 ID。Empty 是唯一无数据起点，第一次规则执行也表示为 CoarseComb。绿色框是独立实例记录，虚线指向共享模板，不是模板的子节点。具体事实与 equality 只在实例及其明确父实例间传播。</p>
<p><a href="shared_templates.svg" target="view">模板 / 实例分层图</a><a href="shared_templates.native.svg" target="view">完整原生图</a><a href="results.json">全部数据</a><a href="../../research/legacy_tier1.egg">.egg IR</a></p>
<iframe name="view" src="shared_templates.svg"></iframe>
<p>这是手工给定 R10/R15 接口的共享与隔离实验，尚未自动从 tier-0 历史导入。全图的 Occurrence ID 不进入 Comb 的 hashcons 键。</p>''')

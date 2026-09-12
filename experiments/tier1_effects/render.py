"""Print readable context projections and untruncated native node/edge exports."""
import html
import json
from pathlib import Path
import subprocess

OUT=Path(__file__).resolve().parent
r=json.loads((OUT/'results.json').read_text())
cases={'concrete':('R10 提公因子 → R15 乘积求导',r['concrete']['analysis']),
       'union':('Equality effect 解锁',r['union']['analysis']),
       'redundancy':('局部依赖冗余',r['redundancy']),
       'before':('Effect 到达前',r['dynamic']['before']),
       'after':('Effect 到达后',r['dynamic']['after'])}
def q(s): return json.dumps(str(s),ensure_ascii=False)
def term(t):
    op=t['op']
    if op.startswith('literal:'):return op[8:]
    return op+('('+', '.join(map(term,t['args']))+')' if t['args'] else '')
def effect(e):
    if e['kind']=='equal':return ' = '.join(map(term,e['args']))
    if e['kind']=='node':return 'Node '+term(e['term'])
    return e['name']+'('+', '.join(map(term,e['args']))+')'
def render(name,lines):
    dot=OUT/(name+'.dot');svg=OUT/(name+'.svg')
    dot.write_text('\n'.join(lines)+'\n')
    subprocess.run(['dot','-Tsvg',str(dot),'-o',str(svg)],check=True,timeout=60)
for key,(title,data) in cases.items():
    assert data['serialization_complete']
    lines=['digraph G {','graph [rankdir=LR,bgcolor="#fafaf7",pad=0.3,nodesep=0.45,ranksep=0.8];',
           'node [fontname="Arial",fontsize=12,shape=box,style="rounded,filled",color="#62706f"];',
           'edge [fontname="Arial",fontsize=10,color="#637577"];',f'label={q(title+" · Rule-comb 上下文 / effect 视图")};labelloc=t;fontsize=20;']
    ids={c['context'] for c in data['contexts']}
    for c in data['contexts']:
        i=c['context'];a=c['application'];kind=a['rule'] if a else 'Join' if c['parents'] else 'Entry'
        label=f'C{i} · {kind}\n'+('ready' if c['ready'] else 'pending')
        if a:
            b=json.loads(a['binding'])['binding']
            label+='\n'+', '.join(f'v{k}={term(v)}' for k,v in b)
            if key=='concrete':label+='a=x, b=2, c=3' if a['rule'].startswith('R10') else 'a=x, b=s=2+3'
        lines.append(f'c{i} [label={q(label)},fillcolor="'+('#dcefe5' if c['ready'] else '#ffe2b7')+'"];')
        for slot,p in enumerate(c['parents']):
            assert p in ids;lines.append(f'c{p} -> c{i} [label="ctx {slot}"];')
        groups=[('given',c['given_data'])]+([('requires',a['requirements_data']),('produces',a['produced_data'])] if a else [])
        for role,es in groups:
            for j,bundle in enumerate(([es] if es else []) if key=='concrete' else [[e] for e in es]):
                ident=f'e{i}_{role}_{j}';text='\n'.join(map(effect,bundle))
                if role=='produces' and not c['ready']:text+='\nplanned / unavailable'
                lines.append(f'{ident} [label={q(text)},shape=note,fillcolor="#edf0fa"];')
                if role=='requires':lines.append(f'{ident} -> c{i} [style=dashed,label="requires"];')
                else:lines.append(f'c{i} -> {ident} [style=dotted,label={q(role)}];')
    for p in data['proposals']:
        ok=p['certified_redundant_for_use'];label=f"omit C{p['removed']}: "+('certified for this use' if ok else 'rejected: indirect dependency')
        lines.append(f'c{p["old"]} -> c{p["candidate"]} [constraint=false,color="'+('#168454' if ok else '#bd3939')+f'",penwidth=2,style=dashed,label={q(label)}];')
    render(key,lines+['}'])
    graph=data['native_egraph'];nodes=graph['nodes']
    native=['digraph G {','graph [rankdir=LR];node [shape=box,fontname="Arial",fontsize=9];edge [fontsize=8];']
    for ident,n in nodes.items():
        op=n['op'];short=op if len(op)<=70 else op[:67]+'…'
        label=short+'\n'+n['eclass']
        native.append(f'{q(ident)} [label={q(label)},tooltip={q(op)},color="'+('#168454' if n['eclass'].startswith('Ctx-') else '#687780')+'"];')
        for j,child in enumerate(n['children']):
            assert child in nodes,(ident,child)
            native.append(f'{q(ident)} -> {q(child)} [label="{j}"];')
    render(key+'.native',native+['}'])
    (OUT/(key+'.native.json')).write_text(json.dumps(graph,ensure_ascii=False,indent=2)+'\n')
options=''.join(f'<option value="{k}">{html.escape(t)}</option>' for k,(t,_) in cases.items())
page='''<!doctype html><html lang="zh"><meta charset="utf-8"><title>Tier-1 · Rule-comb 分析图</title>
<style>body{margin:24px;background:#fafaf7;color:#263637;font:16px system-ui}select,a{margin-right:16px}iframe{width:100%;height:78vh;border:1px solid #cad6d3;background:white}p{max-width:1000px;line-height:1.6}</style>
<h1>Tier-1 · Rule-comb 分析图</h1><p>这里显示的是分析 rule comb 的 tier-1 元层图，不是 tier-0 数学表达式图。节点 C0/C1 等表示组合上下文，effect 描述其对 tier-0 的事实要求与产出。当前实例由手工契约构建，尚未自动导入 tier-0 历史。绿色为 ready，橙色为 pending；虚线说明前提或候选替代，点线说明 effect。原组合保留，冗余证书仅针对当前使用。</p>
<p><select id="case">OPTIONS</select><select id="mode"><option value="">Rule-comb 上下文 / effect 视图</option value=".native">完整 tier-1 原生图</option></select><a id="svg" target="_blank">单独打开 SVG</a><a id="json" target="_blank">完整原生 JSON</a></p>
<p id="explanation"></p><iframe id="view" title="tier1 graph"></iframe><p>完整图保留所有导出节点与边；超长节点标签仅缩短显示，完整内容保存在 tooltip 和 JSON 中。内部关系表的 Unit 输出不代表 Ctx 被 union。</p>
<script>const c=document.getElementById('case'),m=document.getElementById('mode');function show(){document.getElementById('explanation').textContent=c.value==='concrete'?'输入：Diff(x, x*2+x*3)。u=x*2，v=x*3，p=u+v，s=2+3，q=x*s，d=Diff(x,p)。Mul(a,b,out) 的最后一项是结果引用。R10 产生 q 并证明 p=q，随后 R15 才能沿同一输入匹配。':'';let stem=c.value+m.value;document.getElementById('view').src=stem+'.svg';document.getElementById('svg').href=stem+'.svg';document.getElementById('json').href=c.value+'.native.json'}c.onchange=m.onchange=show;show()</script></html>'''.replace('OPTIONS',options)
(OUT/'index.html').write_text(page)
print('Rendered',len(cases),'context views and',len(cases),'complete native graphs')

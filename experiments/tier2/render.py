"""Self-contained inspector for native tier-2 extension candidates and translations."""
import json,html
from pathlib import Path
OUT=Path(__file__).parent
m=json.loads((OUT/'math.json').read_text());n=json.loads((OUT/'math_native.json').read_text());a=json.loads((OUT/'affine.json').read_text());an=json.loads((OUT/'affine_native.json').read_text())
occ={x['event']:x for x in m['occurrences']};ext={x['name']:x for x in m['extensions']}
tier1=json.loads((OUT.parent/'tier1_extract/extraction.json').read_text());names={x['eclass']:x['name'].lstrip('$') for x in tier1['definitions']}
def esc(x):return html.escape(str(x))
parts=['''<!doctype html><meta charset="utf-8"><title>Tier-2 扩展规律</title><style>
body{font:16px system-ui;background:#fafaf7;color:#20383a;max-width:1200px;margin:32px auto;padding:0 20px}p{line-height:1.7}pre{white-space:pre-wrap;overflow-wrap:anywhere;background:#eaf0f2;padding:16px}table{border-collapse:collapse;width:100%}td,th{padding:12px;border-bottom:1px solid #ccd5d3;text-align:left}details{border-bottom:1px solid #ccd5d3;padding:12px}summary{cursor:pointer}input{padding:12px;width:80%}a{color:#09696b}.note{border-left:4px solid #bf8438;padding:12px;background:#fff3dc}</style>
<h1>Tier-2：从组合到扩展规律</h1>
<p><a href="fractal.html">打开精简 Fractal 轨道视图：trigger → 1 → 2 → 3 → …</a></p>
<p>Math 路径读取现有 native tier-1；递推路径通过专门的加法递推接口适配器。这里分别展示有限复用证据、坐标变换和系数检查，不把候选自动当作通用快捷规则。</p>
<p><a href="../../rules/tier2.egg">Tier-2 IR / 规则</a> · <a href="math.egg">Math 输入图</a> · <a href="affine.egg">学到的坐标算子与观察</a> · <a href="README.md">边界与重现</a></p>
<h2>递推：从前 5 步预测更深展开</h2>
<table><tr><th>样例</th><th>学到的 Δ(m,a)</th><th>触发深度</th><th>留出步预测</th><th>原规则系数检查</th></tr>''']
for r in a:
    parts.append(f'<tr><td>{esc(r["name"])}</td><td>{esc(r["delta"])}</td><td>{r["trigger_depth"]}</td><td>{r["heldout_pass"]}/{r["heldout"]}</td><td>{esc(r["certificate"]["status"])}</td></tr>')
parts.append('</table><p>warmup 在训练段跳过 2 步启动 effect 后找到稳定增量；这仍是带阶段条件的候选，不能忽略早期规则分支。unit 学到 m+a=n，从而得到 f(n)=f(m)+(n−m)，条件是 m 可由允许的展开到达。changing 的后半段改变增量，native tier-2 找到 8 个反例。</p><p class="note">端点等价不等于全部 effect 相同。Translate / Compose / Power 只表示坐标变换；中间节点与 union 并未被删除。无限阶结论依赖整数算术、加法结合律以及每一步 guard，原生 i64 运行还须避免溢出。</p>')
for r in a:
    parts.append(f'<details><summary>{esc(r["name"])} 的族、条件和证据</summary><pre>{esc(r["family"])}</pre><pre>{esc(json.dumps(r["certificate"],ensure_ascii=False,indent=2))}</pre></details>')
reduction=json.loads((OUT/'reduce.json').read_text())
parts.append('<h2>显式 Reduce 与解析表达式提取</h2><p><a href="../../rules/reduce.egg">Reduce datatype、归约规则与成本</a> · <a href="reduce.egg">样例</a> · <a href="reduce.json">原生提取结果</a></p><p>Reduce(fold, count, terminal) 成本 100；普通算术节点 1，除法/幂 2。相同 e-class 中优先选择更便宜的解析表达式，原 Reduce 节点仍保留。HigherRule 需要明确的 ReductionInput，尚未自动推导 R23 的累积摘要。</p>')
for row in reduction['examples']:
    parts.append(f'<details><summary>{esc(row["name"])} · cost {row["before_cost"]} → {row["after_cost"]} · {"解析表达式" if row["closed_form"] else "保留 Reduce"}</summary><pre>{esc(row["before"])}</pre><pre>{esc(row["after"])}</pre></details>')
higher=json.loads((OUT/'higher_native.json').read_text())['higher_rules']
parts.append('<h2>规则级 HigherRule(k, R, ctx, binding)</h2><p>这些是 native tier-2 对已有连续 smooth 路径折叠出来的有限幂，不只是坐标变换的 Power。R 包含稳定的接口更新和源规则 effect。coarse 注入留在起始 ctx 中，不被默默省略。</p><p><a href="../../rules/higher.egg">幂折叠规则</a> · <a href="higher.egg">输入</a> · <a href="higher_validation.json">展开验证</a></p>')
for h in higher:
    links=' '.join('<a href="../tier1_extract/index.html#'+t.lstrip('$')+'">'+t+'</a>' for t in h['represents'])
    parts.append(f'<details><summary>HigherRule({h["count"]}, {h["operator"]}, {h["ctx"]}, binding)</summary><pre>{esc(h["egg"])}</pre><p>展开回原组合：{links}</p></details>')
s=m['summary'];parts.append(f'<h2>真实 Math tier-1</h2><p>{s["occurrences"]} 次应用 → {s["extensions"]} 种扩展描述。深度 &gt;3 的 {s["heldout_occurrences"]} 次应用中，{s["heldout_seen_schema"]} 次复用浅层已有描述。不是内存压缩率或独立工作负载泛化率。</p>')
parts.append(f'<p>native tier-2 找到 {len(n["repeats"])} 条同接口、同父端口的连续重复子路径；最长 {max((x["length"] for x in n["repeats"]), default=0)} 次。{n["counts"]["InjectionEdge"]} 条依赖边的消费端需要 coarse 边界；这只是候选注入位置，不是已证明的 trigger。</p><h3>长度至少 3 的重复路径</h3>')
for p in sorted(n['repeats'],key=lambda x:-x['length']):
    if p['length']<3:continue
    e=ext[occ[p['end']]['extension']];anchor=names[occ[p['end']]['template']]
    parts.append(f'<p><a href="#{e["name"]}">{e["name"]} / {e["rule"]}</a>：事件 {p["start"]} → {p["end"]}，父槽 {p["slot"]}，连续 {p["length"]} 次。<a href="../tier1_extract/index.html#{anchor}">对应 tier-1 组合</a></p>')
parts.append('<h3>扩展字典</h3><input id="query" placeholder="筛选 R23 / ext_0001"><p>Schema 保留源规则前提、构造和 union；ParentRole 保留父序号、输出 AST 位置和类型。所有频次都是观察次数。</p>')
for e in m['extensions']:
    parts.append(f'<details class="extension" id="{e["name"]}" data-key="{e["name"]} {e["rule"]}"><summary>{e["name"]} · {e["rule"]} · {len(e["events"])} 次 · 浅层 {e["train_depth_le_3"]} / 深层 {e["heldout_depth_gt_3"]}</summary><pre>{esc(json.dumps({k:v for k,v in e.items() if k!="events"},ensure_ascii=False,indent=2))}</pre></details>')
parts.append('''<script>document.getElementById('query').oninput=function(){let q=this.value.toLowerCase();for(let d of document.querySelectorAll('.extension'))d.hidden=!d.dataset.key.toLowerCase().includes(q)};function focus(){let e=document.getElementById(location.hash.slice(1));if(e){e.open=true;e.scrollIntoView()}}onhashchange=focus;focus();</script>''')
(OUT/'index.html').write_text('\n'.join(parts))

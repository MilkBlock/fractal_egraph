"""Browse whole-combination lowering from the selected native tier-1 graph."""
from pathlib import Path
import json,html
OUT=Path(__file__).parent
r=json.loads((OUT/'extraction.json').read_text())
combined=json.loads((OUT/'combined.json').read_text())
whole={c['name']:c for c in combined['combinations']}
def esc(s):return html.escape(str(s))
parts=['''<!doctype html><meta charset="utf-8"><title>Tier-1 → combined tier-0 rules</title>
<style>body{font:16px system-ui;margin:28px;background:#fafaf7;color:#263637}summary{cursor:pointer;padding:12px}details{border-bottom:1px solid #cad3d3}pre{white-space:pre-wrap;overflow-wrap:anywhere;padding:16px;background:#edf2f5}input{width:70%;padding:10px}p{line-height:1.6}.pair{display:grid;grid-template-columns:1fr 1fr;gap:18px}</style>
<h1>Tier-1 → 整条 combined rule</h1>
<p>先显示整条组合的 tier-0 LHS/RHS：沿已提取的 tier-1 父引用解释 relative binding 和输出端口，保留中间构造及 union。下面保留 tier-1 表达式与末端原规则供核对。额外入口等式属于显式前提；需要中途重新匹配的实例单独列出。</p>
<p><a href="combined.egg">整条 combined rules .egg</a> · <a href="validation.json">原生验证结果</a> · <a href="existing_combs.egg">带逐节点对照注释的 tier-1 .egg</a> · <a href="tier0_rules.egg">独立 tier-0 原规则字典</a></p>
<input id="search" placeholder="查找：R23、comb_0003 …"><p id="count"></p>''']
stats=combined['summary']
parts.append(f'<p>{stats["comb_classes"]} 个已有 Comb · {stats["classes_with_lowering"]} 个可降级 · {stats["rules"]} 条整组合规则 · {stats["lowered_occurrences"]} 个已降级实例。其余保留边界说明，不生成猜测规则。</p>')
for d in r['definitions']:
    name=d['name'];rid=d.get('tier0_rule_id','Empty');anchor=name.lstrip('$')
    c=whole[name]; fused=[]
    for k,v in enumerate(c['variants']):
        steps=' → '.join(x['rule'] for x in v['steps'])
        fused.append(f'<h3>整条 combined rule · variant {k}</h3><p>{esc(steps)}（父依赖 DAG 的执行次序） · {len(v["occurrences"])} 个实例 · {v["guard_count"]} 个额外入口等式</p><pre>{esc(v["egg"])}</pre>')
    if c['other_occurrences']:
        reasons=sorted({x.get('reason',x['status']) for x in c['other_occurrences']})
        fused.append('<p><b>以下实例未导出成单条规则：</b>'+esc('; '.join(reasons))+'</p>')
    fused=''.join(fused)
    parents=' '.join(f'<a href="#{esc(p.lstrip("$"))}" onclick="document.getElementById(\'{esc(p.lstrip("$"))}\').open=true">{esc(p)}</a>' for p in d['parent_combs'])
    parts.append(f'<details id="{esc(anchor)}" data-search="{esc(name+" "+rid)}"><summary>{esc(name)} · {esc(d["kind"])} · tier-0 {esc(rid)}</summary><p>父组合：{parents or "无"}</p>{fused}<div class="pair"><div><b>tier-1 已有表示</b><pre>{esc("(let "+name+" "+d["expression"]+")")}</pre></div><div><b>对应的 tier-0 原规则</b><pre>{esc(d.get("tier0_definition","Empty 不对应规则执行").replace(chr(92)+"n",chr(10)))}</pre></div></div></details>')
parts.append('''<script>let rows=[...document.querySelectorAll('details')];function filter(){let q=document.getElementById('search').value.toLowerCase(),n=0;for(let r of rows){r.hidden=!r.dataset.search.toLowerCase().includes(q);if(!r.hidden)n++}document.getElementById('count').textContent=n+' 个已有组合节点'}document.getElementById('search').oninput=filter;filter();function focusHash(){let r=document.getElementById(decodeURIComponent(location.hash.slice(1)));if(r){r.open=true;r.scrollIntoView()}}window.addEventListener('hashchange',focusHash);if(location.hash)focusHash();else if(rows[1])rows[1].open=true;</script>''')
(OUT/'index.html').write_text('\n'.join(parts))
# Reuse the exact original Math datatype; no schedules or source program inputs.
source=Path('experiments/annotated_export/math_microbenchmark/combined.egg').read_text()
datatype=next(line for line in source.splitlines() if line.startswith('(datatype Math '))
rules=json.loads((OUT/'tier0_rule_dictionary.json').read_text())
used={d['tier0_rule_id'] for d in r['definitions'] if 'tier0_rule_id' in d}
text=['; Original tier-0 rule definitions referenced by the extracted tier-1 graph.',datatype]
for name in sorted(used,key=lambda n:int(n[1:])):text.extend(['; '+name,rules[name]['definition'].replace('\\n','\n')])
tier0_text='\n'.join(text)+'\n'
(OUT/'tier0_rules.egg').write_text(tier0_text)
(OUT.parent/'comb_order'/'ranked.egg').write_text((OUT/'combined.egg').read_text())
print('Mapped',sum('tier0_rule_id' in d for d in r['definitions']),'Comb definitions to',len(used),'source rules')

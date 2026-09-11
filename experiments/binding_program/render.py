"""Readable component definitions, wiring laws, and per-combination decompositions."""
import json
from pathlib import Path
from collections import Counter
base=Path(__file__).parent
p=json.loads((base/'math.json').read_text())
def term(t):
    if 'hole' in t:return f"${t['hole']}:{t['sort']}"
    op=t['op'];args=t['args']
    if op.startswith('in:'):return 'i'+op[3:]
    if op.startswith('out:'):return 'o'+op[4:]
    if op.startswith('assign:'):return 'o'+op[7:]+' := '+term(args[0])
    if op=='Eq':return 'require '+term(args[0])+' ≡ '+term(args[1])
    if op.startswith('materialized:'):op=op[13:]+'@witness'
    return op+('('+', '.join(map(term,args))+')' if args else '')
def wiring(t):return '; '.join(map(term,t['args'][2]['args']))
lines=['# Math BindingMap 构件与关系','',
 f"{p['macro_classes']} 个源位置组合类 → {p['unique_wiring_relations']} 个归一化端口关系。{p['verified_occurrences']} 次历史实例通过连接验证。",'',
'这些关系保留类型、未赋值的 consumer 端口和嵌套等价约束。物化节点的值来自原生见证；不是从原输入独立重算出的结果。共享 wiring 不代表原始 rule 的 guard 或图更新效果等价。','',
'## 发现的纯路由关系','', '| 第一映射 | 第二映射 | 合成结果 |','|---|---|---|']
for e in p['binding_composition_relations']:
    result='Identity' if e['identity'] else e['result_existing_id'] or wiring(e['result'])
    lines.append(f"| {e['first']} | {e['second']} | `{result}` |")
lines+=['','B0 是 `(i0,i1) → (i1,i0)`；B67 是 `i0 → (i0,i0)`。所以 B0;B0 为恒等路由，B67;B0 仍为 B67。证明来自端口代入，不是运行时值偶然相等；但没有将此等式提升为完整 e-graph 更新过程的等价。','','## 构件选择','', '| 构件 | 引入时替换次数 | 扣除定义成本后减少的 AST 节点 |','|---|---:|---:|']
for r in p['learning']:lines.append(f"| {r['id']} | {r['uses_in_unique_map_corpus']} | {r['ast_node_gain_after_definition_cost']} |")
for c in p['components']:
    lines += ['',f"### {c['id']}",'','```text',term(c['pattern']),'```']
lines+=['','这些是有类型的语法展开宏，尚未证明为递归/fractal rule。F 引用其他 F 时构成无环定义图；每一轮选择后都展开回原始 BindingMap 并比较完全相等。新调用仍需检查作用域和接口约束。','','## 描述长度','', '| 表示 | AST 节点单位 |','|---|---:|']
d=p['description_length']
for label,key in [('124 个原始映射','raw_maps'),('归一化去重 + 类别引用','unique_maps_plus_class_references'),('构件库 + 分解 + 类别引用','component_corpus_plus_definitions_plus_class_references')]:lines.append(f"| {label} | {d[key]} |")
lines+=['','这是映射表示的 AST 节点计数，不是实际字节、egraph 内存或完整证明存储。类型签名-only 的压缩不计入主实验；无过滤语法压缩对照的独立映射库成本为 '+str(p['unrestricted_syntax_control']['cost_nodes'])+' 节点，说明更高的纯语法压缩率不自动意味着更有意义的规律。','','## 全部组合的分解','', '| 原排名 | BindingMap | 归一化映射 | 未赋值 consumer 端口 | 构件表达式 |','|---:|---|---|---|---|']
for c in p['classes']:
    lines.append(f"| {c['rank']} | {c['binding_map_id']} | `{wiring(c['normalized'])}` | {c['boundary_consumer_ports']} | `{term(c['component_decomposition'])}` |")
lines+=['','## 同一 wiring 下的不同 rule 组合','']
groups={}
for c in p['classes']:groups.setdefault(c['binding_map_id'],[]).append(c)
for name,cs in groups.items():
    if len(cs)<2:continue
    lines.append(f"- {name}：排名 "+', '.join(str(c['rank']) for c in cs))
    for c in cs:lines.append(f"  - `{c['shape']}`")
(base/'math.md').write_text('\n'.join(lines)+'\n')

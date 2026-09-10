"""Render the reproducible frequency/continuation report from native trace statistics."""
import json
from pathlib import Path
base=Path(__file__).parent
p=json.loads((base/'cyk.json').read_text())
lines=['# CYK 组合结构频率与后继','',
'本表按真实 consumer 事件计数；不是 compiled combined rule 的执行次数。原程序和仅给规则命名的程序均完整通过三个作用域中的检查。',
'', '规则：R0 = 终结符识别/叶子建树；R1 = span 组合；R2 = 解析树组合。',
'', '| Rule | 捕获候选 | guard 后存活 | 直接 Inserted 行 | 真实 union |', '|---|---:|---:|---:|---:|']
for name,r in p['rule_labels'].items():lines.append(f"| {name} | {r['matches']} | {r['survived']} | {r['direct_inserted_rows']} | {r['committed_unions']} |")
lines+=['','## 按观察频率排列','','`p0/p1` 是不同 producer 实例；`P#0/P#1`、`B#0/B#1` 保留同表读取槽位，不能任意交换。完整键值见证在 JSON 中。',
'', '| 排名 | 组合结构 | 次数 | consumer 产生新事实/union | 被后继消费的实例 | 三个作用域次数 | 后继边数 |', '|---:|---|---:|---:|---:|---|---|']
for r in p['motif_rankings']:
    scopes='/'.join(str(r['scopes'].get(str(i),0)) for i in range(3))
    edges=', '.join(f'{k}: {v}' for k,v in r['next_rule_counts'].items())
    lines.append(f"| {r['rank']} | `{r['shape']}` | {r['observed_occurrences']} | {r['productive_consumer_occurrences']} | {r['continued_instances']} | {scopes} | {edges} |")
lines+=['','## 组合之后继续组合','', '以下统计的是共享 producer→consumer 事件的后继结构，不是已经证明可收缩的宏规则。一个实例可以有多个后继，所以边数不是概率。','']
for r in p['motif_rankings']:
    lines.append(f"- `{r['shape']}`")
    for target,n in sorted(r['next_motif_counts'].items(),key=lambda x:(-x[1],x[0])):
        lines.append(f"  - → `{target}`：{n} 条后继边")
lines+=['','## 怎样解释','',
'频率 15 的结构并不一定比频率 14 的结构更有生成价值。例如 `[R0:P#0 + R1:P#1] → R1` 发生 15 次，但仅 8 次产生新事实；`[R0:P#0 + R0:P#1] → R1` 发生 14 次、14 次均产生新事实，且有 21 条后继消费边。',
'', '这里发现了两套相似的递归模式：span 的 R1 与树结构的 R2，都呈现“两个基础结果结合、基础结果与递归结果结合、两个递归结果结合”。这是待进一步验证的规则组合文法，不能直接把 R1 和 R2 判成语义等价。',
'', '三个作用域是同一文件的不同输入/文法配置，不是独立训练集和测试集。当前没有实施频率驱动的选择策略，实际编译宏规则执行数为 0。',
'', '原生规则定义：','']
for name,r in p['rule_labels'].items():lines += [f'### {name}', '', '```egg',r['definition'],'```','']
(base/'cyk.md').write_text('\n'.join(lines))

"""Native paired R23 interventions; render outcomes without assuming the hypothesis."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'experiments/r23_intervention'
subprocess.run(['cargo', 'build', '--release', '--bin', 'r23_intervention'], cwd=ROOT, check=True)
try:
    subprocess.run(['target/release/r23_intervention',
                    'experiments/annotated_export/math_microbenchmark/combined.egg',
                    str(OUT / 'results.json')], cwd=ROOT, check=True, timeout=300)
except (subprocess.TimeoutExpired, subprocess.CalledProcessError):
    (OUT / 'results.json').write_text(json.dumps({'status': 'incomplete'}))
    raise
r = json.loads((OUT / 'results.json').read_text())
assert r['status'] == 'complete' and r['reference_unchanged']
lines = ['# R23 暂停点配对干预', '',
         f"固定参考图：原始 11 轮，{r['reference_enodes']:,} 个 e-node。共 {len(r['samples'])} 个配对快照。", '',
         '| 快照轮次 / 入口 | 原始调度深度 | 连续局部深度 | R1 后局部深度 | 净增节点：原始 / 局部 / R1后局部 |',
         '|---|---|---|---|---|']
for s in r['samples']:
    branches = [s['original_schedule']] + [b['checkpoints'] for b in s['interventions']]
    assert all(len(b) == 4 for b in branches)
    depths = ['/'.join(str(c['completed_depth']) for c in b) for b in branches]
    net = ' / '.join(f"{b[-1]['net_enode_change']:,}" for b in branches)
    lines.append(f"| {s['snapshot_round']} / {s['orientation']} | {' | '.join(depths)} | {net} |")
coverage = sorted({b[-1]['reference']['reference_covered_enodes']
                   for s in r['samples']
                   for b in [s['original_schedule']] + [i['checkpoints'] for i in s['interventions']]})
lines += ['', '四次机会后的目标族固定参考图覆盖数：' + ', '.join(map(str, coverage)) + '。', '',
          '深度向量对应 1、2、3、4 次 R23 机会。原始分支每次运行全部规则；局部分支只执行一个指定 binding。',
          'R1 分支额外执行一次合法的局部乘法交换，是额外 effect 对照，不是已确定的必要修复。',
          '节点增长是可见构造器行数的净变化，不是 Inserted 事件数。全局调度同时做其他工作，差额不等于压缩收益。',
          '参考覆盖只计目标 R23 链的显式 LHS/RHS，不计接口内部结构，不比较整条干预分支的所有效果。']
(OUT / 'comparison.md').write_text('\n'.join(lines) + '\n')

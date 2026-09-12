"""Measure exact union coverage, without enumerating every combined binding."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'experiments/coverage_breadth'
subprocess.run(['cargo', 'build', '--release', '--bin', 'coverage_breadth'], cwd=ROOT, check=True)
try:
    subprocess.run([
        'target/release/coverage_breadth',
        'experiments/annotated_export/math_microbenchmark/combined.egg',
        str(OUT / 'results.json'),
    ], cwd=ROOT, check=True, timeout=600)
except (subprocess.TimeoutExpired, subprocess.CalledProcessError):
    if (OUT / 'results.json').exists():
        result = json.loads((OUT / 'results.json').read_text())
        result['status'] = 'incomplete'
        (OUT / 'results.json').write_text(json.dumps(result, indent=2) + '\n')
    raise
result = json.loads((OUT / 'results.json').read_text())
assert result['status'] == 'complete'
assert result['original_rows_unchanged']
print('Basic joint:', result['basic_joint_nodes'])
print('Expanded joint:', result['expanded_joint_nodes'])
print('Combined marginal:', result['combined_added_joint_nodes'])
previous = json.loads((ROOT / 'experiments/tier0_probe/results.json').read_text())
n = result['original_enodes']
assert previous['original_enodes'] == n
lines = ['# 规则族广度覆盖对照', '', f'相同最终原图：{n:,} 个节点。以下均为去重覆盖，不是压缩率。', '',
         '| 范围 | LHS | RHS | 两侧并集 |', '|---|---:|---:|---:|']
def cell(count):
    return f'{count:,} ({count / n:.5%})'
old = previous['lhs_rhs_joint_unique_coverage']
lines.append(f"| 此前三个模式 | {cell(previous['side_totals']['lhs']['unique_nodes'])} | {cell(previous['side_totals']['rhs']['unique_nodes'])} | {cell(old)} |")
for prefix, label, joint in [('basic', '24 条基础规则', result['basic_joint_nodes']), ('basic_plus_combined', '基础 + 124 条组合规则', result['expanded_joint_nodes'])]:
    lhs = result['groups'][prefix + '_lhs']['final_nodes']
    rhs = result['groups'][prefix + '_rhs']['final_nodes']
    lines.append(f'| {label} | {cell(lhs)} | {cell(rhs)} | {cell(joint)} |')
lines += ['', f"组合规则相对基础规则的联合新增覆盖：{result['combined_added_joint_nodes']:,} 个节点。", '',
          '组合导出的 LHS 保留中间连接事实，因此 LHS 的边际增加可能来自基础规则 RHS 已覆盖的节点。',
          '基础 Add/Mul 交换律各自覆盖整个对应算子表；联合覆盖接近完整不代表存在有效的结构替换方案。', '',
          '未覆盖节点按算子计数：`' + json.dumps(result['uncovered_by_operator']) + '`。']
(OUT / 'comparison.md').write_text('\n'.join(lines) + '\n')

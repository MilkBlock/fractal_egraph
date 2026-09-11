"""Finite 1/2/4/8-step comparison on the original final native graph."""
import json
from pathlib import Path
import subprocess

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/"experiments/integration_family"
subprocess.run(["cargo","build","--release","--bin","integration_family"],cwd=ROOT,check=True)
seed='(Integral (Mul (Cos (Var "x")) (Var "x")) (Var "x"))'
subprocess.run(["target/release/integration_family","experiments/annotated_export/math_microbenchmark/combined.egg",seed,str(OUT/"results.json")],cwd=ROOT,check=True,timeout=180)
result=json.loads((OUT/"results.json").read_text())
lines=["# 分部积分自组合的有限覆盖", "", "单位：最终原图中去重后的 LHS∪RHS 节点；每列固定入口。", "",
       f"| 深度 | 固定 binding | 固定入口 e-class（{result['entry_class_seed_count']} 个 binding） | 全部 LHS 入口对照 |", "|---|---:|---:|---:|"]
for i,depth in enumerate([1,2,4,8]):
    values=[result[k]["checkpoints"][i]["joint_unique"] for k in ["fixed_binding","fixed_entry_eclass","all_lhs_starts_control"]]
    lines.append(f"| {depth} | {values[0]} | {values[1]} | {values[2]} |")
lines += ["", "固定入口 e-class 的新增覆盖（相对上一行）："+", ".join(str(r["new_since_previous_checkpoint"]) for r in result["fixed_entry_eclass"]["checkpoints"])+"。", "",
          "```mermaid", "flowchart LR", '  A["a₀=Cos(x), b₀=x"] --> B["a₁=Diff(x,a₀), b₁=Integral(b₀,x)"]',
          '  B --> C["a₂=Diff(x,a₁), b₂=Integral(b₁,x)"]', '  C --> D["继续检查现有图中的后继，最多 8 层"]', "```", "",
          "只通过查询查找后继；缺少 RHS 或 union 等价关系时停止，不生成任何假想节点。"]
(OUT/"comparison.md").write_text("\n".join(lines).rstrip()+"\n")

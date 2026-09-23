# egglog 3.0 free-join 与 bag 成本实验

本目录研究如何把仓库已经提取的公共 producer→consumer 结构用于 **查询规划**。它不生成快捷规则，也不把 coarse egraph 替换成 ripened egraph。后两种做法会重新逐项匹配或逐项导入，通常绕不开 free join 已经批量完成的工作。

## 基线与初步结果

`math-microbenchmark.egg` 与 egglog v3.0.0 发布源码中的测试相同。这里运行的是真实 egglog 3.0 runtime，不是独立原型。release 单线程运行 9 次，每种模式丢弃第一次预热；2026-09-23 的中位数为：

| 模式 | wall | search | merge | rebuild |
|---|---:|---:|---:|---:|
| 默认 tree decomposition | 432.2 ms | 178.0 ms | 63.0 ms | 176.5 ms |
| 全局 `--no-decomp` | 408.5 ms | 170.0 ms | 60.0 ms | 165.0 ms |

本机样本中 single-bag 的 wall 中位数低约 5.5%，但这还不是一种通用策略。它说明当前 planner 的结构性 min-fill/pathwidth 判据没有充分计入中间结果与物化成本。

## 要建模的成本

对候选 bag `b`，第一版使用可测量的加性模型：

```
C(b) = α·scan_rows
     + β·hash_probes
     + γ·intersection_candidates
     + δ·materialized_rows
     + ε·message_width·materialized_rows
     + ζ·downstream_reuses
```

对整个分解 `D`：

```
C(D) = Σ C(b) + Σ edge_lookup_cost - reuse_credit(D)
```

`reuse_credit` 只能在同一轮/时间戳语义下确实复用已物化关系或编译计划时计入。仓库的公共结构数据可提供以下先验：

- producer/consumer 源端口决定 join 键，避免把同名 constructor 的不同位置混合；
- `observed_occurrences` 估计一个 bag 形状出现频率；
- `productive_consumer_occurrences / observed_occurrences` 估计产出率；
- binding alias 与边界位置约束候选是否真能共享。

这些频率不能直接当 cardinality。实际 cardinality、stage 输入行数、探测数和物化行数必须由 runtime 采样校准。尤其不能把 logical matches 写成 committed mutations。

## 实施顺序

1. 在 v3 planner 中为每个候选分解同时计算 single-bag 与 decomposed 的静态特征。
2. 在 `FusedIntersect`、`Intersect` 和 `FusedIntersectMat` 记录 scan/probe/candidate/materialize 计数。
3. 用多个程序拟合系数，并以保守阈值选择分解；不确定时保留现有 planner。
4. 将公共结构目录作为跨规则的 plan-cache key 和先验，而不是新增 egglog rule。只有键、约束、时间戳模式一致时才共享物化结果。

现有 `stage-info` 报告只给计划形状、耗时和最终 match 数，尚不足以分离上述各项。`analyze_stage_report.py` 会把真实报告整理为每条规则的 stage 结构、调用次数、match 数和耗时，作为下一步内核计数器的基线。

## 复现

```bash
cargo build --release \
  --manifest-path vendor/egglog-3.0.0-src/Cargo.toml
python3 run_bench.py
vendor/egglog-3.0.0-src/target/release/egglog \
  --report-level stage-info \
  --save-report results/stage-report.json \
  math-microbenchmark.egg
python3 analyze_stage_report.py results/stage-report.json \
  -o results/stage-summary.json
```

`vendor/egglog-3.0.0-src` 是官方 `v3.0.0` tarball 的本地展开目录。构建产物和原始大报告不应提交；提交紧凑摘要和可复现脚本。

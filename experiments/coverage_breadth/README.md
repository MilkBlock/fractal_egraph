# 全规则族广度覆盖对照

运行 math_microbenchmark 原始 11 轮，然后对最终原生 egglog 图查询。
包含导出文件中的全部 24 条基础规则与 124 条 combined witness bundles。
组合规则仍在未调度的 `__viz_combined` ruleset；没有执行快捷规则扩图。

## 测量含义

- 分母：原始图的 1,047,896 个非 subsumed 构造器表行。
- 分开求 LHS / RHS 覆盖集合，再对两侧求并集。
- RHS 保留导出 JSON 指定的 binding 连接约束；这些辅助约束自身不计入 RHS 覆盖。
- 接口变量所指的整个子图不会自动计入覆盖。
- `basic_plus_combined_*` 是基础规则与组合规则的累计并集；不是组合规则单独的覆盖率。
- `pattern_results.added_nodes` 是按 atom 数、规则名排序后的边际贡献，不能当作该规则独立覆盖量或使用频率。
- 本轮没有计算实例打包、结构删除或字节压缩率，也没有证明任何无限 fractal rule。

## 如何避免算重及实例笛卡尔积

所有覆盖以原始表行 ID 的集合记录。基础规则覆盖后，组合规则只需要确认剩余行是否被覆盖。

1. 单 atom 模式可直接投影原始表，并检查常量、重复变量和等式约束。
2. 每个 atom 的表名、常量和重复变量给出候选行上界；上界全部已经覆盖时，严格证明该 atom 没有边际贡献。
3. 候选较多时，用 egglog 原生规则生成该 atom 的投影 marker，仅记录相关行列。
4. 候选不超过 32 行时，用原生 existential `check` 锚定现有行，检查整个模式是否成立。

锚点通过带类型的常量 primitive 和全局绑定引用现有 Value，不用 extract 重建表达式。
辅助标记仅在克隆图中生成。查询后逐表逐行检查原始构造器行未改变。
`no_additional_coverage_by_atom_bound` 表示已证明新增覆盖为零，不表示此模式没有匹配。

结果仍是当前图上的结构匹配，不是历史上已提交的 rule apply 次数或因果来源证明。

## 完整 RHS action 的包含证明

组合 RHS 逐 action 与单 action 的基础 RHS 做 alpha 等价比较：只允许变量重命名，
保留重复变量关系、构造器、常量及全局名字。若每个 action 均有对应基础规则，
则组合查询加上的连接约束只能缩小匹配集合；各 action 覆盖的并集必然包含于已测出的基础 RHS 并集。

这种情况记录为 `no_additional_coverage_by_complete_action_containment`，并列出证明所用的基础规则。
这是严格的零边际证明，不是跳过未知结果；也不提供组合实例数量。
当前实现只以单 action 的基础规则建立证明，避免错误地丢掉多 action 基础 RHS 的联合约束。
不满足这一充分条件的组合仍走原生查询。

直接查询版本曾在复杂组合 RHS 上超过 600 秒；该次结果未作为完整结果使用。
本目录完整结果由包含证明优化后的版本重新从原始程序生成。

## 复现与验证

```sh
python3 experiments/coverage_breadth/run.py
cargo test --bin coverage_breadth
```

运行中结果标记 `running`，失败或超时由 runner 标记 `incomplete`，完整结果标记 `complete`。
测试在小图上将投影、锚定和单 atom 优化与完整实例 probe 比较，覆盖重复变量和等式约束，
并继承原有覆盖、外部引用及有限族终止测试。

覆盖对照见 [comparison.md](comparison.md)，逐模式边际贡献及证明见 [results.json](results.json)。

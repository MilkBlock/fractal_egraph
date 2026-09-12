# Tier-1 rule-comb 分析 IR（简化版）

- **tier-0**：运行数学 rewrite 的普通表达式 e-graph。
- **tier-1**：分析 tier-0 已产生的 rule comb；Apply 表示已经见证的步骤，不是等待执行的计划。

当前例子由手工契约构造，并在独立的原生 tier-0 中验证 R10/R15；尚未自动导入完整 trace 或连接 babble。

## 核心表示

```
Ctx = Entry(id)
    | Join(Ctx, Ctx)
    | Apply(parent, rule_id, binding, requirements, produced_effects)

Effect = HasFact(predicate, typed_arguments) | Equal(typed_value, typed_value)
```

没有 Known、ValueKnown、Grounded、Ready 或 pending Apply。构造器本身就标识上下文，
Apply 的历史产出直接传播。调用方须提供实际支撑；Rust 导入接口拒绝不完整 binding
或缺少输入支撑的记录，不在图中保存等待补齐的节点。这是导入一致性检查，不是执行调度。
coarse 的外部支撑通过 Join 接入，不能因为某个局部边界无法独立支撑它，就把实际发生的步骤判成未执行。

## 留下的分析

- `Provides(ctx,effect)`：组合可提供的历史效果。
- `EqAt`、`ArgsEqual`、`Satisfies`、`AllSatisfied`：同一个上下文内的等价及整组 binding 条件检查。
- `SupportsUse(ctx,apply)`：这个上下文足以支撑该 Apply；用于分析边界与替代依赖，不是 Ready 的改名。
- `RedundantForUse(old,replacement_parent,removed)`：省去一个 Join 分支后，另一分支仍独立满足该使用。

原 Smooth / DominatesForUse / AlternativeSupport 的重复条件归并到 SupportsUse。
没有 Ctx union、全局 producer 删除或预测执行。冗余提案仅引用已有父上下文，不创建虚构的历史 Apply。
适配器以不可变上下文 DAG 检查 Independent；替代分支间接依赖 removed 时拒绝证书。

等价仍按上下文隔离，不以全局 Value union 实现。构造器输出相等不会反向统一子参数。
Entry 是调用方供应的外部公理根；不得把 removed 的产出伪装成独立 Entry。
公理新增只改变 SupportsUse 等分析关系，已记录 Apply 的发生状态不变；回滚/撤回需新建分析实例。

## 文件

- `tier1_rule_comb_ir.egg`：原生 tier-1 分析规则。
- `tier1_rule_comb_example.egg`：R10→R15 的已见证组合，只有 C0 输入、C1 R10、C2 R15。
- `tier0_math_example.egg`：实际数学规则及执行前后的原生检查。
- `src/tier1_effects.rs`：有类型的契约实例化、导入检查、独立性检查及导出适配器。

R10 将 p=x*2+x*3 与 q=x*(2+3) 合并，R15 通过这一支撑执行乘积求导。
`SupportsUse(C1,C2)` 成立，`SupportsUse(C0,C2)` 不成立；这描述的是依赖边界，
不是 C2 尚未执行。图中不再放入过去那个 pending R15 分支。

## 复现与图形

```
python3 experiments/tier1_effects/run.py
cargo test --test tier1_effects
```

从仓库根目录运行，以解析 .egg 的 include；图形渲染需要 Graphviz `dot`。
`index.html` 提供五个分析视图及完整原生图。两幅支撑变化图中的 Apply 都已经发生，
只比较某个局部上下文后来能否独立支撑该使用。绿色为 Apply，蓝色为输入/Join，不表示 ready 状态。
`*.native.json` 与 `*.native.svg` 保留全部 serializer 节点与 child 边；上下文/effect 视图是可读投影。

7 个测试覆盖：真实 equality 支撑、不泄漏等价、局部冗余与间接依赖拒绝、
拒绝不完整历史记录、只更新支撑分析、类型/端口检查、R10/R15 原生对照及独立 .egg 执行。
当前仍为小规模语义基础设施，不是 Math 压缩率证明。

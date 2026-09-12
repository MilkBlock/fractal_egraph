# Tier-1：共享组合模板与独立实例证据

`Entry` 已移除。主 IR 现在分成两层：

```
Comb = Basic(rule_id)
     | SmoothComb(parents, rule_id, relative_ports)
     | CoarseComb(parents, rule_id, partial_relative_ports)

Instance = Occurrence(unique_event_id, Comb)
```

Comb 的子结构只有规则、父模板及结构化端口；具体值、执行 ID、effect 均在实例层。
即使两次执行的数据不同，只要连接模板相同，就引用同一个 Comb。
父模板使用 Parents 列表，保留多 producer DAG，不强制串成单链。

## Relative binding

- `ParentPort(parent_index, output_index, sort)`：从指定父实例的输出取值。
- `External(slot, sort)`：coarse 组合中的外部数据接口。
- `Make(operator, child_ports, sort)`：用已有 materialization 见证解释构造型转移。

外部槽在导入前按首次出现顺序编号。Rust 的 `external_ports` 提供此规范化：
名字不同可以共享，但同一外部值的重复使用不会与两个不同值混淆；sort 冲突会被拒绝。
Make 不生成数据，只有 `Materialized(instance,op,args,value)` 已提供时才能解析。
`.egg` 示例已规范编号；尚未实现任意 binding 表达式的理论等价归一化。
SmoothComb 的 binding 应仅使用父端口/已见证构造关系，CoarseComb 可以使用 External；
当前要求导入方遵守该分类，不会仅凭构造器名字证明外部依赖不存在。

## 实例与支撑

`ParentAt` / `OutputAt` / `ExternalAt` 存实际连接和具体值，不作为 Comb 的构造参数。
实例 ID 必须在一次分析中全局唯一（跨 trace/scope 时应先重新编号）。
ParentAt 只有与 ParentTemplate 一致才会获得 LinkedParent，继而传播输出与效果。
导出时拒绝同一实例端口存在不同赋值的矛盾 witness。

`Produced` 是历史产出；`ExternalFact` 是该实例实际用到的边界支撑。
`Provides`、`EqAt`、`SupportsUse` 均按 Instance 隔离，只沿明确的父实例连接传播，
不会因为共享 Comb 而把两个实例的事实混起来。没有 Known/Ready 或待执行状态。
Requires 是该实例的 ground 前提；SupportsUse 是效果覆盖分析，不是允许执行的门槛。

局部冗余提案仍在实例层：ProposedRemoval + SupportsUse + Independent 给出 RedundantForUse。
Independent 必须由实际依赖证据支持；Rust 导出器检查 ParentAt 祖先关系并拒绝循环支撑。
这不是模板等价证明，不能据一组实例证书 union 所有同模板组合。
新版未提供通用自动冗余候选枚举；不会迁移旧版具体 Ctx 证书冒充模板级结论。

## 结果与验证

`tier1_rule_comb_example.egg` 用 R10→R15 的固定接口展示两组具体数据：

- 第一组：x、2+3、dx；
- 第二组：y、4+5、dy。

两组各有一个 R10 与一个 R15 occurrence，共 **4 个实例、2 个 Comb 模板**：
一个 Basic(R10)、一个共享 CoarseComb(R15)。重新构造同一个 coarse 模板的等式检查通过。
前提来自各自的乘积表示、外部 Diff 行和父实例的 equality，不会跨实例泄漏。

测试另行验证：不同实例分别只有 P/Q 时不能合并满足 P∧Q；equality 不泄漏；
外部端口别名规范化；Make 需要真实 materialization；错误父模板不提供 binding；
矛盾端口赋值被拒绝；间接依赖不能伪称 Independent。tier-0 数学例子仍通过原生检查。

这是手工供应的接口/证据实验，尚未自动提取完整 tier-0 历史、学习 fractal 递推或接入 babble。
模板共享的计数不等于整体内存压缩率，实例证据仍有存储成本。

## 文件与复现

- `tier1_rule_comb_ir.egg`：当前主 IR，直接由 egglog 执行。
- `tier1_rule_comb_example.egg`：模板共享与实例隔离的可执行检查。
- `tier0_math_example.egg`：普通表达式 e-graph 的 R10/R15 例子。
- `src/tier1_effects.rs`：端口规范化、证据检查与导出。

```
python3 experiments/tier1_effects/run.py
cargo test --test tier1_effects
```

从仓库根目录运行，渲染需要 Graphviz。`index.html` 显示模板/实例分层图及完整原生图。
旧版带 Entry 的生成图已替换；模板图中的虚线 instance-of 不是模板的 child 边。

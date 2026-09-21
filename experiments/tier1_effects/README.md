# Tier-1：共享组合模板与独立实例证据

`Entry` 与 `Basic` 已移除。唯一的 `(Empty)` 是无数据、无效果的组合起点。主 IR 现在分成两层：

```
Comb = Empty
     | SmoothRuleComposition(ParentCombs, RuleId, RelativeBinding)
     | CoarseRuleComposition(ParentCombs, RuleId, PartialRelativeBinding)

Instance = Occurrence(unique_event_id, Comb)
```

Comb 的子结构只有规则、父模板及结构化端口；具体值、执行 ID、effect 均在实例层。
即使两次执行的数据不同，只要连接模板相同，就引用同一个 Comb。
父模板使用 ParentCombs 列表，保留多 producer DAG，不强制串成单链。

## Relative binding

- `RuleId` 是独立类型，以 `(Rule "R15")` 构造，不能直接传裸 String。
- `ParentPort(parent_index, output_index, sort)` 返回 LocalPort。
- `Make(operator, RelativeBinding, sort)` 返回 LocalPort；其子项递归限制为局部端口。
- `RNil / RCons` 构成 RelativeBinding，只能包含 LocalPort，供 SmoothRuleComposition 使用。
- `Local(LocalPort)`、`External(slot, sort)`、`MakePartial(operator, PartialRelativeBinding, sort)` 返回 PartialPort。
- `PNil / PCons` 构成 PartialRelativeBinding，供 CoarseRuleComposition 使用。

例如：

```lisp
(SmoothRuleComposition $parents (Rule "R15")
  (RCons (ParentPort 0 0 "Math") (RNil)))

(CoarseRuleComposition $parents (Rule "R15")
  (PCons (Local (ParentPort 0 0 "Math"))
    (PCons (External 0 "Math") (PNil))))
```

External 不能直接或通过嵌套 Make 流入 SmoothRuleComposition；这由原生 egglog 类型检查拒绝，
不依赖运行后标记。局部 Make 和 MakePartial 均需要实例中的 Materialized 见证，不生成数据。
这是 binding 语法的约束，不替代对外部 effect 来源与支撑完整性的验证。
PartialRelativeBinding 也允许全部为 Local 的表达式；本次未增加自动 coarse→smooth 重写。

外部槽在导入前按首次出现顺序编号。Rust 的 `external_ports` 返回 PartialRelativeBinding：
名字不同可以共享，同一外部值重复使用与两个不同值不会混淆，sort 冲突会拒绝。
`.egg` 示例已规范编号；尚未实现任意 binding 表达式的理论等价归一化。

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

两组各有一个 R10 与一个 R15 occurrence，共 **4 个实例、3 个 Comb 节点**：
一个全局 Empty、一个 CoarseRuleComposition([Empty],R10,External 0/1/2)、一个共享 CoarseRuleComposition(R15)。重新构造同一个 coarse 模板的等式检查通过。
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


## 等价归一化与 babble 入口

新增独立 `tier1_equivalences` ruleset，分析器在普通支撑闭包后运行它，再更新闭包。
`ToLocal`/`LocalPart` 识别没有 External 的 binding 子结构。

可证明的表示等价是：

```
MakePartial(op, fully_local_args, sort)
  = Local(Make(op, corresponding_local_args, sort))
```

两侧查找同一实例、同一 op/参数/结果 sort 的 Materialized 见证，所有父端口索引及别名保持。
该 partial-port union 通过 congruence 合并相应的 CoarseRuleComposition 表示，但不删除任何 effect 或支撑。
`LocalBindingView` 提供局部 binding 视图；**不会将 CoarseRuleComposition 直接 union 成 SmoothRuleComposition**，
因为局部端口不能单独证明额外 effect 需求不存在。实例冗余证书也不会直接变成全局模板等价。

`learn.py` 把实际导出的 Comb 图交给现有 babble adapter。所有根共用一张定义表，
共享祖先只编码一次，实例 ID/具体数据不进入学习程序；binding 内部仍采用结构化 AST。
遇到无法编码为有限 DAG 的循环会拒绝，不能无限展开。

```
python3 experiments/tier1_effects/run.py
python3 experiments/tier1_effects/learn.py
python3 -m unittest discover -s experiments/tier1_effects -p test_learn.py
```

这是小语料入口验证：训练和测试来自同两个模板的不同实例，不能宣称独立泛化。
修正逐根重复定义的计费后，训练/测试均为 **55→55 AST 节点，0 个新增库抽象**，无损展开通过。
初次逐根编码的 80→69 包含重复序列化收益，不作为超过原有 DAG 共享的证据。
`babble_corpus.json` 与 `babble_results.json` 保留可复现输入及结果。
尚未把全部实际 tier-0 组合历史自动接入这套 IR，也尚未生成满足完整 effect/interface 契约的依赖删除等价式。

当前 13 项 Rust 语义测试、2 项 DAG 编码测试通过。

导出器拒绝 `Occurrence(id, Empty)`，防止把 Empty 再用成带数据身份的 Entry。

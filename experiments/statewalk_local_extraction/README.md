# 从 eggcc StateWalk 到局部规则组合 Extraction

本文记录对本地 `check_statewalk` 仓库的代码阅读，并给出适用于 `egg_layout` 的迁移边界。结论是：公共子结构应成为局部 extraction 候选，而不是被直接写回全局 e-graph。StateWalk 可以作为局部选择器的骨架，但必须先建立类似 RVSDG region 的结构约束。

## eggcc 的实际流水线

eggcc 并不直接在 CFG 或整个 e-graph 上运行一个普通 tree extractor：

1. Bril CFG 被 restructure 成结构化控制流。
2. RVSDG 用 `BasicOp`、`If/Gamma`、`Theta` 表示值、分支和循环；操作数只引用 region 参数或更早节点的某个输出。
3. RVSDG 被编码为共享 DAG。控制流成为 `If/Switch/DoWhile`，状态边成为 `StateT`，region 叶子带 `Assumption` context。
4. egglog 产生等价候选后，Tiger 把包含状态类型的 e-class 标为 effectful。
5. 全图按 effectful 边切成 region。一个 enode 的第一个 effectful child 留在当前 region，其他 effectful children 成为递归提取的子 region。
6. 每个 region 先用 StateWalk DP 选择 effectful spine，再在保留下来的图上做共享感知的 greedy DAG extraction。

这不是“先选每个 e-class 最便宜的 enode”。effectful 选择会改变哪些纯 e-class 已经可以构造，因此 DP 状态必须包含可提取集合。

## StateWalk 的状态和转移

源码中的一个 DP 项保存：

```text
(cost, persistent_extractable_set, predecessor, current_effect_class, chosen_enode)
```

初始状态是 region 的 state argument。常量、参数和所有依赖已经满足的纯节点先进入可提取闭包。

从 effectful e-class `u` 选择父 effectful enode `n` 到达 `v`，需要：

```text
all children(n) are already extractable
```

选择成功后：

1. 增加 `n` 的局部代价；
2. 将 `v` 标为可用；
3. 沿纯节点的反向依赖递减未满足 child 计数；
4. 对计数归零的纯节点递归闭包；
5. 以 `(v, live_extractable_set)` 合并等价 DP 状态。

优先队列按累计成本扩展，所以它本质上是在隐式状态图上运行 Dijkstra。到达 region root 后回溯得到 effectful statewalk。

实现使用持久化计数数组、持久化 bitset、Zobrist 式集合哈希和 pure-saturation cache，避免为每个 DP 分支复制完整集合。liveness 只保留当前位置到 root 仍可能使用的纯 e-class，因此两个历史不同但未来等价的状态可以合并。satellite 优化则跳过从同一 effect 状态出发、不会改变活跃可提取集合的大量近似重复分支。

## RVSDG IR 中真正重要的性质

对本项目有用的不是 `Gamma`、`Theta` 的名字，而是以下不变量：

- **显式端口**：`Operand::Arg(i)` 和 `Operand::Project(port,node)` 完整表达 region 输入与多输出。
- **拓扑引用**：节点只能引用更早节点，纯数据部分天然是 DAG。
- **显式状态边**：Print、memory 和 function state 与普通值区分，不能由纯重写随意重排。
- **结构化 region**：分支和循环有明确输入、输出和内部 context。
- **共享身份**：相同纯子表达式使用同一个 `Rc<Expr>`，提取成本按 DAG 共享计算。
- **context 属于叶子/region**：`InFunc/InIf/InSwitch/InLoop` 描述表达式在哪个作用域有效，不靠全局名字猜测。

## 映射到 saturated rule composition

建议新增一个只用于 extraction 的局部 IR，不修改 tier-0 等价闭包：

```text
LocalRegion
  inputs:  typed boundary ports
  outputs: observable facts/equalities/visibility
  root:     effect frontier
  candidates: LocalCandidate[]

LocalCandidate
  class:       alternative-result class
  effect_in:   required committed frontier
  effect_out:  produced committed frontier
  requires:    local values/facts
  provides:    local values/facts
  body:        original rule fragment | shared motif | ripened body
  context_key: rule environment + visibility + injection obligations
  evidence:    source applications and replay certificate
  cost:        measured execution-cost components
```

对应关系如下：

| eggcc | egg_layout |
|---|---|
| RVSDG region argument | coarse composition typed boundary / `RipenInput` |
| effectful e-class | 可观察的 rule effect、union、visibility 或 rebuild frontier |
| pure e-class | 局部事实、binding 计算、公共 producer→consumer 子结构 |
| alternative enode | 原始规则路径或一个已验证的公共结构候选 |
| Assumption | rule environment、coarse context、注入义务和可见性 |
| subregion | 独立 effect 分支或嵌套 composition |
| extracted DAG | 选中的局部执行计划，不是新的全局等式 |

公共子结构必须以带端口的 candidate 进入与原始展开相同的 alternative class。它不是自动可用的宏；只有 boundary、alias、context 和 effect contract 都一致时才是合法候选。

## 建议的第一版算法

只处理满足以下条件的 coarse region：

1. 可观察 effect 可以沿依赖关系得到唯一主 spine；
2. 其他 effect 分支可以切成独立子 region；
3. 所有公共结构都有固定 typed ports；
4. 候选的 effect 与原始路径已经由 replay 证明一致。

DP 状态使用：

```text
(frontier_id, live_available_signature)
```

转移条件是候选的 `requires` 已可用，转移后把 `provides` 加入集合并计算纯闭包。使用仓库已有的增量 Zobrist 指纹作第一层 key，精确 bitset/有序集合确认相等。不同历史只要当前 frontier、活跃事实、接口 alias 和 visibility 相同，就可以共享 continuation。

第一版代价不沿用 eggcc 的算术指令常量，而应测量：

```text
candidate_cost = trigger_lookup
               + binding_materialization
               + expected_join_probes
               + expected_rebuild_work
               + residual_obligation_cost
               + code_size_amortization
```

共享 motif 的收益只在实际选择一次、被多个输出或后续候选引用时扣除：

```text
region_cost = sum(selected candidate costs)
            - verified_shared_work
            + boundary_and_fallback_cost
```

## 不能直接照搬的部分

- rule applications 是偏序，未必像 StateT 一样天然线性；强行线性化会丢失并行候选或制造任意顺序。
- ripened egraph 是候选的证明与实现来源，不应整体导入 tier-0。
- 相同公共形状不代表相同 effect、visibility 或 injection contract。
- eggcc 的 cost 面向生成程序的执行成本；这里必须使用真实 matcher、join、rebuild 和缓存命中数据。
- 如果一个 region 有多个不可分离的 effect frontier，StateWalk 的二维状态不足，应回退到 beam/ILP，或者继续细分 region。

## 实施次序

1. 从现有 coarse/smooth 和 dependency evidence 导出 `LocalRegion`，只读，不改变执行。
2. 把原始路径注册为必有 fallback candidate，再加入已验证公共结构候选。
3. 实现结构检查，只有单 effect spine 的 region 进入 StateWalk 原型。
4. 记录 state width、候选接受/拒绝原因、提取成本和真实执行成本。
5. 用 math microbenchmark 比较原始局部计划与 extracted plan；成功后再考虑将选择结果反馈给 tier-0 调度。

这一顺序保留了完整原规则作为回退，也让公共子结构只改善局部候选质量。它不会因为一次全局替换而污染其他 rule context。

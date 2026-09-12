# Combined rule 优先级与简单分块

分数定义为 `enode_num(rhs) / max(1, enode_num(lhs))`，越大越优先。
Empty 是零效果单位，分数为 0；非 Empty 的空 LHS 也会单独标记，分母保护为 1。

本次输入是之前已经通过 native tier-1 检查的真实 Math **前 6 轮**导入结果：
1,529 个应用实例，1,389 个 Comb 模板节点（包括 Empty）。不是完整 11 轮图。
`.egg` 的 Parents/Ports 等元数据构造节点不参与这次分割。

## LHS / RHS 口径

对每个候选子图，用其实际执行实例作为见证：

- LHS：无法由块内已验证 producer 支撑的实际 constructor 读取。
- RHS：块内所有原生 RHS constructor 写入的逻辑节点，包括已存在而 deduplicate 的结果，
  不仅是净新增 Inserted；中间产出也计入 RHS。
- 去重键为 `(scope, constructor, typed input values)`，不把输出 e-class ID 当作节点身份。
- 同一个模板在块内有多个不同执行实例时，保留这些实例；不能把定义共享误当成执行共享。
- 同一候选的根模板有多个实例时，逐个测量，以最小分数作为保守排序分数，记录观测范围。

这些是**历史见证中的逻辑 enode 计数**，不是最终快照的 canonical 节点计数或物理内存。
输出展示用 v0/v1 等重命名 Math 值；k0 等保留 primitive 的原始类型/值引用，不假装已经解码数值。
图里省去了端口内部未被 rule 读取的结构。尚未消去所有语义上可推导的 LHS，也未证明输出是一条通用可执行快捷规则。

## 排序和分割

生成最长祖先距离 0…4、最多 32 个模板节点的候选，包含所有单节点候选。
使用最长距离保证窗口在原 DAG 中凸，避免在一个块内跳过中间依赖。
Empty 单独作为零分块，不在所有候选里重复覆盖。

初始分割是全部单节点。每次选择增益最大的可行合并：

```
gain = candidate_score - sum(scores of replaced blocks)
```

只有完整包含现有块、且 gain >= 0 的合并才接受。同分时偏向较大合并以减少块数。
每次还验证收缩后的块间图无环：即使两个块在原 DAG 中分别凸，合在一起的商图仍可能成环。

这是有限候选集上的贪心近似，不是全局最大值证明。满足：

- 所有模板节点恰好覆盖一次；
- 块间依赖无环；
- 总分不低于单节点分割基线。

6,437 个候选的实测结果：1,192 个分块；总分 1,389.667 → 1,393.833。
最高优先级候选为 R23 的 5 步链，LHS=2、RHS=30、score=15。
比值之和不一定强烈鼓励大块：最高单块分数与最优总分不是同一个目标，保留单节点基线正是为此。

## 查看与复现

打开 `index.html`，先看已选分块，再看最高分的前 100 个候选。
每项可展开查看 LHS → RHS 与具体见证事件。
`ranking.json` 保存全部排序、已选块、增益与覆盖验证。

```
python3 experiments/comb_order/order.py
python3 -m unittest discover -s experiments/comb_order -p test_order.py
```

`input.json` 是固定、可复现的原生导入图快照，保留上述计数所需的实际读写和模板/实例对应，
记录完整来源文件的 SHA-256。无需再连接或运行 babble。
如要从新的原生导入结果重建输入：

```
python3 experiments/comb_order/order.py --profile PROFILE.json --manifest MANIFEST.json --tier1 TIER1.json
```

排序没有执行组合规则，也没有修改 tier-0 或 tier-1 图，仅给出顺序和候选分割。

## 用旧 API 还原 Egglog

```
cargo run --release --bin ranked_comb_egg
python3 experiments/comb_order/order.py
cargo test --bin ranked_comb_egg --bin rule_combine
```

`ranked.egg` 包含按优先级排序的可导出定义；只注册 `__ranked_combs` ruleset，不自动执行。
`egg_export.json` 保存每一项的源码、验证信息或未支持原因。浏览页可展开看代码。

复用的是 `src/bin/rule_combine/compose.rs` 的组合算法和 `Rule::command`，没有另写组合器。
保留旧默认 32 AST 节点上限；导出明确使用 512 的有限上限，并允许最终项不变但中间有 effect 的组合。
两步的内部上下文情形另尝试旧 `contextual_candidates`；R10→R15 的该 API 路径有独立原生测试。
来源连接来自原生 witness 的读写 AST 位置，未找到唯一连接的 DAG 不猜测位置。

还原不能只保留外层最终表达式：旧 command 的外层 context equality 不一定推出每个内部
rule-apply 根的 equality。因此导出器按见证位置恢复每一步的局部 union。
每对局部 lhs/rhs 都检查为对应源规则的结构代换；并在原生 egglog 的新符号实例上检查
最终结果和所有局部 equality。这不是对所有历史实例重新匹配，也不是执行性能证明。

全部 6,437 项的结果：

- 2,769 项成功导出，其中 1,381 项多步、1,388 项单步。
- 3,667 项未被旧 API 表达，具体原因留在报告中。
- Empty 作为单位项跳过，不生成 rewrite。
- 全部导出定义原生类型检查通过；195 种不同多步动作形状经过原生实例检查，同形定义复用检查。

最高分 `ranked_000001` 是五步 R23，保存了五个 union 动作。
排序里的 RHS=30 是历史去重 enode 数；导出最终 RHS 展开树有 47 个构造器位置。
这两种计数不混用，注释分别记录 historical_* 与 export_*。文件中的多条定义可能具有相同
符号形状但来自不同排名/边界实例，因此不要把注册所有定义视为已经优化的执行调度。

当前选中的分割中，**1,191 个非 Empty 分块全部导出成功**；另一个块是 Empty 单位。
未支持的 3,667 项属于其他候选，不影响本次选中分割的语法展示。
每项代码可在排序页直接展开查看；历史规则名列表不被当作充分连接证据。

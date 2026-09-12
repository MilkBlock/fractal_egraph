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

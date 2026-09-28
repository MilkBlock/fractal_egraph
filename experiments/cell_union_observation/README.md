# union 在 RipSat cell 之间造出了什么连接

数据来源：已经跑完的观测运行 `out/cell-evolution-complete/{boundary,refinement,multi_parent,unrelated,math}-observe`
（`EGG_LAYOUT_CELL_EVOLUTION=1`，由 `tools/bench/cell_evolution.py` 产生）。本实验**不重跑引擎、不改规则**，
只把已有 catalog 和 `progress.json` 挖开。`math` = `experiments/math_mini3/source.egg` 6 轮 + 固定 bake 库、单线程。

复现：`python3 experiments/cell_union_observation/analyze.py`（输出 `results.json`）。

## 观察到的八件事

**F1 接触图稠密，交互稀疏。** math 里 **826,555** 条共享 Math 值的 loose contact（观察器判定为「不值得展开」），
但只有 **2,631** 个 boundary event 被展开 —— **314 : 1**。负对照 `unrelated`（两个没有任何连接的 cell）
恰好是 1 条 loose contact、0 个 boundary event。所以 union 造出来的是**势**，一条完整物理 LHS witness
才把它变成**事件**。用户说的「没有连接就不会交互」成立，但连接的绝大多数并不导致交互。

**F2 union 在事件窗口里不可见。** 3,211 个 transition pattern 里 trace 能归因的 read-value union = **0**；
`before_aliases` 与 `after_aliases` 在**全部** pattern 相同；`observed_slot_partition_changes` 在 math 和
4 个玩具里都是 **0**。这不是「没有 union」：观察器的过滤条件是「同一 match 事件 + union 两端都在事前已命名槽 +
有 displaced 值」，congruence 的 `f(x)=f(y)` 父节点合并被直接丢弃。内核 `UnionEvent`
（`egglog/core-relations/src/trace.rs:768`）根本没有 cause 字段，action union 与 rebuild/congruence merge
在 `TraceCause::finish_union` 合流。**要区分这两种扰动，必须在 `trace.rs:779`/`uf/mod.rs:459` 加 cause，
或在 `reuse_evolution.rs:379` 重新归因——目前没有。**

**F3 伤口是点状的。** 一次边界事件钉住源 cell 的接口槽数：**1 个 1783 次、2 个 1220 次、3 个 168、4 个 40**
（pattern 数），94% ≤ 2。扰动是单点事件，不是面积事件。

**F4 扇出是重尾的。** 一个事件牵涉的其他 cell 数：median **17**、p90 **144**、max **578**；
**218** 个 pattern 存在单个接口值被 **>200** 个 cell 共享（最大 524）。但按出现次数加权后，
只有约 **7%** 的事件涉及 ≥100 个 cell。hub 是一个**值**，不是 cell。

**F5 两条通道的签名不同。**

| 通道 | patterns | occurrences | 中位(body, external) 读 | 中位 support cell | births/occ |
|---|---:|---:|---:|---:|---:|
| `body_read`（源 cell 提供 body fact） | 1,810 | 38,311 | (1, 1) | 16 | 0.780 |
| `input_refinement`（只读了源 cell 的 RipenInput） | 1,401 | 29,421 | (0, 2) | 23 | 0.819 |

**F6 边界效应 90% 是新增，10% 是重规范化。** 6,805 条 committed Math 行里 **6,094 inserted / 711 updated**。
`updated` 是「已存在的行被 union 后重新 canonicalize」的足迹，也就是等价性回传的可见部分——它只有 10%。

**F7 分支比 < 1。** births per occurrence 全局 **0.797**（body_read 0.780 / input_refinement 0.819）；
四个玩具 fixture 恰好是 **1.0**（12 事件→12 个新 cell）。6 轮的 math 运行是次临界的。

**F8 类型恒定、实例爆炸。** 6 轮里 `families`（cell 类型）**恒为 13**，而 e-class 从 50 涨到 748，
每 match 引擎时间从 **0.026 ms 涨到 1.41 ms（55×）**。飙血发生在实例层，不在类型层。

## 可以利用的特征（假设，未验证）

1. **控制点是 value，不是 cell。** 826k loose contact → 2.6k realized event 说明真正的过滤器是
   「这个共享值是否能被某条已烘规则完整读取」。可以先做这个判定，再决定是否为一次 union 触发传播；
   这比「union 了就重扫所有相关 cell」省 300 倍量级的机会。
2. **高扇出 value 单独限流。** 218 个 hub（单值 >200 个 reader cell，max 524）只占 ~7% 的事件，
   但它们决定了单次传播的重扫上界；把 hub 值的处理拆批或延迟，收益集中在这一小撮上。
3. **边界协议以「发布-插入」为主。** 90/10 的 inserted/updated 比例意味着重规范化是少数路径，
   不必为每次边界事件准备完整的 rebuild 语义。
4. **分支比 <1 是结构性的。** worklist 的收敛不靠预算，而靠拓扑；这支持把 cell 当作有限工作集调度。

## 必须补的观测（现在做不到）

* **union 的 cause 区分**（action union vs congruence/rebuild merge）：内核没有，需要加在
  `trace.rs:779` / `uf/mod.rs:459`。没有它，回答不了「哪种扰动更贵」。
* **全量 per-event log**：现在每个 pattern 只留一个 `example`，其余聚合成计数，所以 826k 条 loose contact
  只有总数、没有边，无法做度分布/连通性分析。

## 边界

* 只有 math_mini3 6 轮 + 4 个玩具，固定 bake 库、单线程、observer 开启；不是通用结论。
* births per occurrence 来自 pattern 级聚合计数（`new_target_occurrences/occurrences`），不是逐事件重放。
* `loose_interface_contacts_not_expanded` 是计数，不是保留的边集。
* F2 的「0 个归因 union」是观察器过滤的结果，不能读成「没有发生 union」——原 README 也这么警告。

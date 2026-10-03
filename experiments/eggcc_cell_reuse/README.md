# eggcc 上的 cell 复用率：**规则驱动 92%，数据驱动 0%**

在 codex 的多 datatype 支持（`6252dc0`）之后，这套 analyze 已经能吃下真实 eggcc 的 trace。
本文用它量回答一个问题：**eggcc 上到底有没有"重复的局部结构"可供 RipSat 复用？**

数据来源：`experiments/multiple_datatypes/run.py` 导出的**真实** eggcc 一次 optimizer pass
（`fact.bril --run-mode egglog --stop-after-n-passes 1`），分析器写出
`observed-transitions.json` + `analysis.json`。本文只读这些产物，**不运行也不修改分析器**。

## 输入 trace（先确认不是玩具）

| 指标 | 值 |
|---|---:|
| logical matches | 4,664 |
| 计入分析的 observed applies | **2,060** |
| 其中 `ObservedCoarseApply` / `ObservedSmoothApply` | 730 / 1,330 |
| 不同 rule 数 | 158 |
| 触及 ruleset 数 | 18（`always-run` 1045、`type-analysis` 247、`terms-helpers` 168、`terms` 123…） |
| 写入的 fact | 2,436 |
| union effects | 194 |
| `updated_rows = true` 的 apply | 189 |
| 外部 fact 依赖 | 758 |
| 有 `parents` 的 apply | 1,954 |
| 读 fact 次数 | 10,304，覆盖 2,152 个不同 fact |

**注意**：这 2,060 个 apply 全部落在 codex 说的那个 audit 拒绝里
（`automatic merge update needs a pre-update table-state witness`），所以它们**没有**被 ripening。
但这不影响本文的结论——`observed-transitions.json` 是发现阶段的产物，它照样记录了
每个 apply 的 `rule` / `binding` / `inputs` / `inserted_facts`。

## 结论：两种"复用"，差了两个数量级

| 复用键 | occurrences | distinct keys | **重复率** | 平均重数 | 最大重数 |
|---|---:|---:|---:|---:|---:|
| rule identity alone | 2,060 | 158 | **92.3%** | 13.04 | 107 |
| (rule, union effects) | 2,060 | 344 | **83.3%** | 5.99 | 95 |
| (rule, reader slot, producer member/output) | 8,869 | 3,034 | **65.8%** | 2.92 | 62 |
| fact id（任意位置） | 10,304 | 2,152 | **79.1%** | 4.79 | 475 |
| **(rule, written rows)** | 2,060 | 1,837 | **10.8%** | 1.12 | 73 |
| **(rule, multiset of read facts)** | 2,060 | **2,060** | **0.0%** | 1.00 | **1** |

读法：

- **"同一个规则被反复触发"是真的**：2,060 个 apply 只用了 158 条规则，平均每条触发 13 次，
  最热的 rule 278（`(Get expr i) → union (Single (Get expr i)) (SubTuple expr i 1)`）触发 107 次。
  事实层面的复用率也有 79%（10,304 次读取只用掉 2,152 个不同 fact，最热的 fact 被读 475 次）。
- **但"同一个数据组合被反复触发"是假的**：`(rule, 读到的 fact 多重集)` 这个键
  **2,060 个 apply 对应 2,060 个不同的键，一次重复都没有**。也就是说
  **没有任何一次 join 是在重复另一条已见过的 join**。写出的行也几乎不重复（10.8%）。

所以在这份 trace 上：

> **cell 复用率（数据侧）= 0%，规则侧复用率 = 92%。**

这个 0% 值得强调，因为它是一个**下界意义上的干净结论**：如果一个 pattern 实例都没重复，
那么任何"把某次 join 的结果存下来给下次用"的机制，在这里都省不掉一次 join。

## 这解释了三件事

1. **为什么既有的 reuse 在 raytrace 上收益低**。cell/view/cover 挂的是**执行侧的 RHS 替换**
   （`reuse_cell_transitions.rs`），而 RHS 只占 1% 的时间（merge 0.377s/49.58s）；
   而占 81% 的 LHS 搜索里，可复用的 join 实例是 0。作用点错位 + 没有可复用对象，双重为零。
2. **为什么 codex 的 2,060 个 cell 全被拒绝，本身也是一个信号**。这些 cell 互相之间
   在"写出来的行"层面只有 10.8% 重复——本来就没有多少可合并的。
3. **为什么"marker/标志性 sort"方向仍然有价值，但要换个定义**。
   92.3% 的规则级重复说明：**同一个规则在大量不同的数据位置上被触发**。
   这不是"复用某个 join 的结果"，而是"**复用某个规则的 join 计划/局部上下文**"——
   也就是把 rule 变成按 cell 键索引的驱动表，而不是把结果缓存起来。

## 对"省搜索"的具体含义

本文数据支持的形态：

- 对 `(rule, reader slot, producer/output)` 这个键（65.8% 重复，平均 2.92 重）做**索引**是有意义的：
  同一个 producer 的同一个输出口被同一条规则在同一个位置上读了近 3 次。把这层索引物化，
  可以把"再扫一遍 producer 表"变成"按 key 取一次"。
- 对 `(rule, 读到的 fact 集合)` 做**结果缓存**没有意义（0% 重复）——这正是之前
  在 raytrace 上试图做的事。

换成一句话：**eggcc 的浪费不是"算了两遍同样的 join"，而是"同一条规则在 2,060 个不同位置上
各算了一遍"**。前者靠缓存（无效），后者靠**更窄的驱动键 + 更好的 join 顺序**（有效）。

## 复现

```sh
# 1) 用真实 eggcc 导出一次 optimizer pass 并分析（写入新目录；历史是 immutable 的）
python3 experiments/multiple_datatypes/run.py \
  --eggcc /Users/mineralsteins/Repos/egg_related/check_statewalk \
  --output /tmp/eggcc-cell-reuse

# 2) 只读测量
python3 experiments/eggcc_cell_reuse/measure_reuse.py /tmp/eggcc-cell-reuse
```

产物：`reuse-rate.json`（本文所有数字的机器可读版本）。

## Caveats

- 单次导出、单次分析；`run.py` 用的是 **debug 构建**的分析器，所以里面的时间不能当性能基准
  （codex 的 README 也这么写）。本文只报计数，不报时间。
- 这 2,060 个 apply **没有经过 ripen**（被 audit 拒绝），所以"cell"本身没有被认证；
  但这影响的是"这些 cell 能不能复用"，不影响本文的**输入统计**。
- 只覆盖一次 pass（`--stop-after-n-passes 1`）。多 pass 会在同一图上继续累积，
  数据侧的重复率可能上升——那是另一个待测问题，本文不做外推。
- 重复率按 occurrence 计，不是按"节省的时间"计；没有做时间外推。

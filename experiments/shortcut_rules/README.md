# shortcut rule 对 vanilla egglog 有没有加速

问题：既然 `(Add (Add a b) c)` 这类模式可以一次性 rewrite 出很多表达式，为什么不把
所有可能性在写 rule 的时候就展开，让一条 rule 顶掉多轮饱和？本实验测的就是这个。

**结论：在 vanilla egglog 上没有加成，反而更慢、更占内存。**看下面「结果」。

引擎是 pristine egglog（`egglog-baseline` = `c7553a6`，上游 `ebba7bb`），
`out/vanilla-commutativity/bin/{pristine,probe}`，无 tracing、无 egg_layout 代码。

## 三种写法

* `generic`（基线）：`(rewrite (Add a b) (Add b a))` + `(rewrite (Add a (Add b c)) (Add (Add a b) c))`，
  迭代到饱和。
* `shortcut`：一条 rule，body 命中 `(= root (Add a (Add b c)))`，head 一次做完三元素
  AC 轨道：3 个 `union (Add a b) (Add b a)` 把二元子和也合并，再加 11 个 `union root …`。
  共 14 个 action。
* `shortcut_naive`：去掉那 3 个二元子和的 union，只罗列 11 个组合。**故意保留的错编码**，
  只用来展示「写出所有项」≠「断言它们相等」，不参与计时对照。

每种的最终闭包都做结构比较：每个 Add e-node 用它两个参数和结果的**叶子集合**标注，
与类编号无关。n=4,6,8,10,12 上 `generic == shortcut` 全部为真；`shortcut_naive` 全部为假。

## 结果（chain，AC 饱和，引擎内 `seconds` 为 5 次中位数）

| n | 写法 | 迭代到饱和 | 逻辑 matches | 引擎中位用时 | Add e-node | e-class |
|---:|---|---:|---:|---:|---:|---:|
| 4 | generic | 4 | 120 | 0.0010 s | 50 | 15 |
| 4 | shortcut | 2 | 68 | 0.0011 s | 50 | 15 |
| 6 | generic | 7 | 3,335 | 0.0025 s | 602 | 63 |
| 6 | shortcut | 4 | 2,572 | 0.0041 s | 602 | 63 |
| 8 | generic | 8 | 68,116 | 0.0201 s | 6,050 | 255 |
| 8 | shortcut | 5 | 56,908 | 0.0292 s | 6,050 | 255 |
| 10 | generic | 9 | 1,245,807 | 0.1354 s | 57,002 | 1,023 |
| 10 | shortcut | 6 | 1,049,470 | 0.2175 s | 57,002 | 1,023 |
| 12 | generic | 9 | 21,271,073 | 2.7447 s | 523,250 | 4,095 |
| 12 | shortcut | 6 | 17,282,098 | 3.6067 s | 523,250 | 4,095 |

* 饱和迭代 **9 → 6**（n=10/12），逻辑 matches **少 16–19%**（n=4 时少 43%），
  但引擎时间 **反而慢 1.3–1.6×**。
* 单次匹配成本：n=12 时 generic 约 129 ns/match，shortcut 约 209 ns/match —— **1.6×**。
  matches 少 19% 抵不过单次贵 60%：少而胖的匹配不划算。
* 峰值 RSS（n=12，`/usr/bin/time -l`，无 graph export，3 次中位数，见 `rss_n12.json`）：
  generic **1.27 GiB**，shortcut **5.26 GiB**，**4.1×**。短时间内在一条 rule 里产出大量
  新项/union，瞬时缓冲比多轮迭代更大。
* 增长曲线：generic 在 n=12 第 8 轮先冲到 556,781 再回落到 523,250；shortcut 单调爬到
  523,250。也就是说 shortcut 消掉了中间过冲，但峰值内存反而更高——高内存来自单轮的
  写入/union 缓冲，不是节点过冲。

## 瓶颈在哪（引擎自带的分阶段计时）

用 `profile_probe.rs` 把程序写成 `(run 1)` 序列，逐轮读取 egglog 自己的 `RunReport`
（默认 `ReportLevel::TimeOnly`）：`search+apply`（跑 query plan + 实例化 action）、
`merge`（把产生的 tuple 灌进表、去重，并沿依赖表传播）、`rebuild`（有新 union 时重建 id）。

| n | 写法 | search+apply | merge | rebuild |
|---:|---|---:|---:|---:|
| 8 | generic | 45% | 37% | 18% |
| 8 | shortcut | 50% | 42% | 8% |
| 10 | generic | 38% | 43% | 19% |
| 10 | shortcut | 60% | 31% | 9% |
| 12 | generic | 21% (**0.59 s**) | 71% (**1.94 s**) | 8% (0.22 s) |
| 12 | shortcut | 45% (1.95 s) | 48% (2.06 s) | 7% (0.31 s) |

n 越大，generic 的瓶颈越明确地落在 **merge**：37% → 43% → 71%。join 本身（per-rule
计时）在 n=12 只有 0.138 s，其中结合律规则 20,406,196 次匹配、交换律 866,614 次。

量级对比：n=12 最终只有 **523,250** 个 Add e-node，却发生 **21,270,810** 次逻辑匹配 ——
**约 40 次候选节点求值，才留下 1 个节点**。所以时间不是花在「找匹配」上，而是花在
把 2000 多万个候选 e-node 灌进表、去重、传播（merge）以及它们引起的 union 之后的重建上。
换句话说：**瓶颈是冗余的节点物化（materialization），不是搜索轮数**。

shortcut 没有减少这个量：matches 只降 19%，但每条匹配的 action 从 1 个变成 14 个
（3 个二元子和 union + 11 个 root union），于是 `search+apply` 每匹配代价涨约 3×，
整体时间反而更差；n=12 峰值 RSS 也从 1.27 GiB 涨到 5.26 GiB。

推论：想提速要减少**被反复求值/去重的候选节点数**（更早去重、避免重复推导），
而不是减少饱和轮数——这正是 shortcut 思路的反面。

## 重复发生在饱和末段，不在边界

n=12 generic 逐轮（matches 来自 profile，净新增 Add 来自 `(print-size)`）：

| 迭代 | matches | 净新增 Add | matches/净新增 |
|---:|---:|---:|---:|
| 1–5 | 56,818 | 42,543 | 0.5 – 1.4 |
| 6 | 471,071 | 150,727 | 3.1 |
| 7 | 2,710,654 | 280,189 | 9.7 |
| 8 | 8,551,217 | 83,322 | **102.6** |
| 9 | 8,796,449 | −33,531 | 净减少（全是重复） |
| 10 | 689,336 | 0 | 只为确认饱和 |

* 前 6 轮（边界传播）只占 **2.5%** 的 matches，1 条新事实约 1 次匹配，重复很小。
* 第 8、9 轮占 **81.5%** 的 matches，净产出 49,791 个节点（**9.5%**）。
* 第 9 轮 8.8 M 次匹配净减少 33.5k 行：congruence 合并把过冲节点收掉，这一轮没有任何净进展。

所以「减少重复」的对象是**末段的匹配实例**，不是规则条数、也不是饱和轮数。
规则大小也不是变量：`comm` 和 `assoc` 都是 1 个 action 的小规则，n=12 时
`comm` 866,674 次匹配、`assoc` **20,408,821** 次——24× 差距来自 body 在稠密图里的
可实例化方式，不是规则大小。shortcut 才是「大规则」，它确实把匹配数降了 19%，
但每条匹配要 14 个 action，于是输在单匹配成本上。

## shortcut_naive 为什么不作数

它 sound 但不去重：n=4 时 Add=92 / e-class=21（generic 50/15），n=12 时 661,256/4,161
（generic 523,250/4,095），matches 也更多（n=12 约 39.5 M），时间 10.5 s。用它做对照会
得到「shortcut 慢 3–4×」，但那是编码不等价的后果，不是 shortcut 本身。

## 复现

```sh
BASELINE=/Users/mineralsteins/.codex/worktrees/native-baseline/egg_layout
python3 experiments/vanilla_commutativity/build.py --baseline "$BASELINE"
python3 experiments/shortcut_rules/build_profile.py --baseline "$BASELINE"
python3 experiments/shortcut_rules/shortcut_bench.py --n 4 6 8 10 12 --repeat 5
python3 experiments/shortcut_rules/profile_phases.py --n 8 10 12
```

原始数字：`results.json`（结构、等价性、逐步曲线、重复计时）、`rss_n12.json`、
`phases.json`（分阶段计时）；运行时脚本也会写到被忽略的 `out/shortcut-rules/`。
`phases.json` 的总时间是带计时的 `(run 1)` 序列，与 `results.json` 的整段 `(run 200)`
略有差异，百分比趋势一致。

## 边界

* 只测 AC on chain，n=4…12；`shortcut` 是「三元素轨道」这一种组合方式，
  没有测四元素及更深的组合（head 会迅速膨胀）。
* matches 是多线程下的逻辑匹配数，不是已提交变更；generic 的 matches 在重复运行间有
  约 0.03% 波动，shortcut 重复运行完全一致。
* 每条配置只在一台机器上测；时间取 5 次中位数，未做配对统计检验。
* 这测的是 vanilla egglog 的规则执行开销，不涉及 egg_layout 的 combined rule /
  ripen 机制；两者结论不能互相搬运。

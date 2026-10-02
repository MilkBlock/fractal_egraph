# raytrace：egglog 各阶段耗时（merge / insert row / extraction）

回答一个问题：raytrace 编译 49.6 s，到底有多少花在 **merge（提交插入的行）**、
**insert row**、**extraction** 上？以及"生成出来的 e-graph 明明不大，为什么这么慢"。

## 结论（先看这个）

**merge / insert row 根本不是瓶颈：三个 pass 合计 0.377 s，占 egglog 时间的 0.96%、
占端到端 49.58 s 的 0.76%。** extraction（eggcc 内嵌的 Tiger pass）是 6.85 s ≈ 13.8%。
真正的大头是 **search + apply（join 枚举 + 把 action 结果 stage 起来）31.73 s ≈ 64%**。

| 段 | 秒 | 占 egglog wall | 占端到端 49.58 s |
|---|---:|---:|---:|
| **search + apply**（join 枚举、match 计数、RHS 结果 staging） | **31.731** | 81.2% | **64.0%** |
| **merge**（= `Table::merge` → `do_insert`/`do_delete`/rehash + union 提交） | **0.377** | 1.0% | **0.76%** |
| rebuild（congruence closure） | 5.660 | 14.5% | 11.4% |
| **extraction**（eggcc 的 Tiger/StateWalk 提取） | **6.848** | — | **13.8%** |
| serialization（e-graph → JSON 交给 Tiger） | 1.717 | — | 3.5% |
| 其余（bril 解析、bril→rvsdg→dag、回程 to_cfg/to_bril 等） | 1.932 | — | 3.9% |
| egglog 合计 | 39.083 | 100% | 78.8% |

单次成本：**5.48 µs / rule match**，**0.54 µs / 真正提交的行**。
所以"慢"不在写表，而在**找到那么多次 match**。

## 为什么"e-graph 不大"这个前提不成立

最终那个 e-graph（`out/raytrace-egraphs/tiger_egraph_0003.json`，即第 3 个 pass 出来的图）：

| 指标 | 值 |
|---|---:|
| enodes | 716,893 |
| eclasses | 543,572 |
| **enodes / eclass** | **1.32** |
| 不同的 op 名 | 18,962 |
| JSON 体积 | 212.4 MB |

两个关键事实：

1. **1.32 enodes/eclass ⇒ saturation 几乎没造出等价类。** 这张图的大小不来自"等价饱和"，
   而来自 tree/DAG encoding 的展开（inlining 之后的项本身就有几十万节点）。
   （作为对比：math-microbenchmark 11 轮后是 1,047,896 enodes / 443,832 eclasses = 2.36 enodes/eclass，
   比 raytrace 这一版还"更饱和"一些。）
   它更接近"一个 70 万行的关系库"，而不是"一个被压缩的 e-graph"。
   这也解释了为什么 merge/union 便宜：**根本没有多少东西可以合并**。
2. 交叉验证：`num_tuples()` 在三个 pass 后是 697,997，dump 里的 enodes 是 716,893 —— 同量级，
   说明 dump 就是 egglog 数据库本身，下面按 op 分类的统计代表真实内容。

op 分布（dump 3，前 12 名）：

| op | 节点数 | 占比 |
|---|---:|---:|
| `lsr-inv` | 122,846 | 17.1% |
| `Smaller` | 72,255 | 10.1% |
| `PotentialExtractedExpr` | 69,649 | 9.7% |
| `TCPair` | 63,060 | 8.8% |
| `BodyContainsExpr` | 50,594 | 7.1% |
| `TermGet` | 31,119 | 4.3% |
| `SubTuple` | 21,246 | 3.0% |
| `ExprIsResolved` | 19,173 | 2.7% |
| `Expr-size` | 19,161 | 2.7% |
| `HasType` / `HasArgType` / `ContextOf` | 各 19,160 | 各 2.7% |
| `Get`（真正的程序项） | 13,341 | 1.9% |
| `Concat`（真正的程序项） | 8,147 | 1.1% |

即：**绝大多数（上表前 12 名合计约 74%）的行是分析/类型/代价模型/提取框架的簿记**（`Smaller`/`TCPair` = 提取代价的全序关系，
`PotentialExtractedExpr`/`ExtractedExpr` = 提取候选与结果，`HasType`/`ContextOf`/`Expr-size` = 类型与代价），
真正的程序项（`Get`/`Concat`/`Bop`……）只占几个百分点。

## merge / insert row 具体是什么

这个 kernel 里规则 RHS 不直接写表：search 阶段把结果 stage 进 buffer，
`merge_all()` 才真正提交（`core-relations/src/free_join/mod.rs:680`，
`RuleSetReport.merge_time` 就是这一段的计时，见 `core-relations/src/free_join/execute.rs:671-673`）。
提交对每张有 pending 更新的表调用 `Table::merge` → `do_delete` + `do_insert` + `maybe_rehash`
（`core-relations/src/table/mod.rs:754-762`）。

所以在这个测量里，**"insert row 的时间"就是 merge_time**，没有第二条写路径。
数据也自洽：三个 pass 净增 697,997 行，花了 0.377 s ⇒ **0.54 µs/行**；表是 hash-consed 的，
写路径不是问题。

对比 match 数：**5,785,630 次 match 只换来 697,997 行**（≈88% 的 match 不产生新行），
而 match 枚举本身花了 31.7 s。这才是时间的去处。

## 时间具体花在哪些 ruleset / 哪些规则（pass 2，search 18.07 s 内部）

pass 2 是 10,482 次 ruleset iteration 的总和（三个 pass 共 23,378 次），
每次 ruleset run 平均 1.7 ms，但每次都要重扫它相关的那几张表。

按 search 排序的主要 ruleset（pass 2）：

| ruleset | search | merge | rebuild |
|---|---:|---:|---:|
| `always-run` | 5.008 | 0.174 | 0.199 |
| `terms-helpers-helpers` | 4.776 | 0.019 | 1.542 |
| `terms` | 2.584 | 0.010 | 0.019 |
| `terms-helpers` | 2.021 | 0.028 | 0.001 |
| `type-analysis` | 1.790 | 0.003 | 0.009 |
| `is-resolved` | 0.469 | 0.002 | 0.004 |
| `subst` | 0.313 | 0.002 | 0.007 |
| `context` | 0.313 | 0.007 | 1.574 |
| `loop-iters-analysis` | 0.171 | 0.000 | 0.000 |
| `error-checking` | 0.162 | 0.000 | 0.000 |

最贵的单条规则（pass 2 前 8）：

| rule（截断） | search+apply | matches | µs/match |
|---|---:|---:|---:|
| `Smaller (TCPair t1 cost1) (TCPair t2 cost2)` + `(= cost1 c ...)` | 1.724s | 554 | **3111.9** |
| `PotentialExtractedExpr expr (TCPair term cost)` + `ExtractedExpr ...` | 1.647s | 42,713 | 38.6 |
| `Smaller (TCPair ...)` + `(< cost1 cost2)` | 1.523s | 68,394 | 22.3 |
| `Smaller (TCPair ...)` + `(> cost1 cost2)` | 1.514s | 75,562 | 20.0 |
| `Bop o e1 e2` + `TCPair t1 c1 = ExtractedExpr e1` | 0.779s | 16,735 | 46.5 |
| `Concat e1 e2` + `ExtractedExpr e1` | 0.659s | 6,161 | 107.0 |
| `Concat x y` + `Expr-size`（求和） | 0.607s | 329,668 | 1.8 |
| `Top o e1 e2 e3` + `ExtractedExpr e1` | 0.570s | 4,070 | 140.0 |

两种截然不同的贵法：

* **便宜但巨量**：`Expr-size`/`Get` 这种「对每个子项累加」的规则，1.8 µs/match，
  但一条就 329,668 次、另一条 956,220 次 match —— 合计上亿次枚举的一部分，累计起来很可观。
* **昂贵但命中极少**：`Smaller (TCPair …)` 那三条是**代价排序关系的两两 join**，
  554 / 68,394 / 75,562 次 match 却各花 1.5–1.7 s，其中 554 次那条是 **3.1 ms/match**——
  典型的大表 join 只有极少数命中。这三条 + `PotentialExtractedExpr`/`ExtractedExpr` 一族
  合计 8.42 s，是 pass 2 search 的 **46.6%**（再加上 `Expr-size` 那类巨量枚举就超过一半）。

也就是说：**raytrace 的瓶颈是"提取代价模型（`Smaller`/`TCPair`/`*ExtractedExpr`）的关系 join"，
不是 e-graph 的写入、也不是等价合并。**

## 与之前 Tiger profile 的关系

`experiments/tiger_raytrace_profile/README.md` 测的是 Tiger C++ extractor 内部
（`read_nodes` 50%、`compute_statewalk_cost` 27%、`statewalkDP` 1.2%）。
那份报告是从 `--egraph-out-dir` dump 出来的 JSON 文件起算的；本文的 extraction 6.848 s
是 eggcc 流水线内的实测，两者量级一致。serialization 1.717 s 就是流水线里
"把 70 万行的图写成 JSON"那一步，也正是 Tiger 那边 `read_nodes` 要重新读回来的东西。

## 测量对象与方法

* 二进制：`check_statewalk@ae80f82a` 的 release build，**只加了一个 42 行的只读 reporting patch**
  （`instrumented/dag_in_context_phase_report.patch`），没有改 egglog、没有给被测代码加计时器。
* 数据来源：kernel **自带的** `RunReport`。这个 kernel 默认就是 `ReportLevel::TimeOnly`
  （`egglog-reports/src/lib.rs:17-18`），`EGraph::get_overall_run_report()` 是公开 API；
  patch 只是把它打印出来。search/apply、merge、rebuild 的计时点在
  `core-relations/src/free_join/execute.rs:398,671` 和 `egglog-bridge/src/lib.rs:745`。
* egglog：eggcc 实际链接的 `egglog-experimental` rev `08771f9`（与本地 `egglog/` 同一 core，
  见 `egglog/UPSTREAM.md`），**未做任何修改**。
* 输入：`benchmarks/passing/raytrace/raytrace.rs`（821 行，ARGS 写在文件里），
  `--run-mode=optimize`，单线程 egraph（`new_single_threaded_egraph` 把 rayon 固定成 1 线程）。
  三个 pass 对应 schedule 的三个 step。
* 机器：macOS arm64；跑两次，pass 级数字相差 <0.5%（`results/passes.log` 是第 2 次，
  `results/passes_run1.log` 是第 1 次）。

## 复现

```sh
cd /Users/mineralsteins/Repos/egg_related/check_statewalk
git apply /Users/mineralsteins/Repos/egg_related/egg_layout/experiments/raytrace_phase_time/instrumented/dag_in_context_phase_report.patch
LLVM_SYS_180_PREFIX=/opt/homebrew/opt/llvm@18 \
LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/cbc/lib:/opt/homebrew/opt/coinutils/lib \
  cargo build --release --offline
EGGCC_PHASE_REPORT=1 EGGCC_PHASE_RULES=1 /usr/bin/time -l \
  ./target/release/eggcc --run-mode=optimize benchmarks/passing/raytrace/raytrace.rs \
  >/tmp/raytrace_out.bril 2>results/passes.log
# EGGCC_PHASE_REPORT=1 → 每个 pass 一行 EGGCC_PHASE + 末尾一行 EGGCC_STATS
# EGGCC_PHASE_RULES=1  → 额外的逐 ruleset / 逐规则明细
```

文件清单：

| 文件 | 内容 |
| --- | --- |
| `instrumented/dag_in_context_phase_report.patch` | 42 行只读 reporting patch |
| `results/passes.log` | 完整输出（含逐 ruleset / 逐规则明细） |
| `results/passes_run1.log` | 第一次运行的完整输出 |
| `results/summary.json` | 上面几张表的机器可读版本 |

## Caveats

* `search_and_apply_time` 包含 match 计数：kernel 里 match 数要在 join 之后单独
  `read_matches` 填回去（`free_join/execute.rs:655-663`），所以它是"找 + 记账 + staging"的总和，
  不是纯 join。
* `merge` 只统计 ruleset 的提交阶段；pass 0 的 4.5 s 里还包含 prologue 的 parse + run
  （schema/type 规则），那部分不在任何 ruleset report 里。
* 单进程、单线程、release、系统分配器；peak RSS 1.96 GB。
* `extraction` 是 eggcc 内嵌 Tiger pass 的 wall；`regions=0` 是因为该计量只在特定开关下收集。
* 这里统计的是**实际运行时**（eggcc + 它链接的 egglog），不是 standalone 原型。

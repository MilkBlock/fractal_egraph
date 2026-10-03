# 只用 rule 改动能省多少搜索：raytrace 上 −4.31 s（−8.7%）

在 `check_statewalk@ae80f82a` 的 `dag_in_context/src/utility/terms.egg` 上做**纯 rule/声明级**改动
（不动 engine、不动 Rust），用 `experiments/raytrace_phase_time` 的 phase report 量前后差。
两次运行都是 release eggcc、`--run-mode=optimize benchmarks/passing/raytrace/raytrace.rs`、单线程 egraph。

## 改了什么（`terms.egg.diff`，净 −8/+5 行）

1. **折叠 tie 分支**：`Smaller` 的解析原本是三条规则（`<` 取 t1、`>` 取 t2、`=` 取 t1），
   改成两条（`<=` 取 t1、`>` 取 t2）。语义完全等价（`<=` 覆盖 `<` 和 `=`）。
2. **删掉 negative-cost panic 规则**：`(PotentialExtractedExpr e (TCPair t cost)) (< cost 0) → panic`。
   实测它 **0 命中却花了 0.569 s**（把 69,649 行 `PotentialExtractedExpr` 全扫一遍）。
   cost 只由两条 base 规则（`TermConst`/`TermArg`，字面量 1）和子 cost 相加构成，非负是归纳成立的。

## 结果

| 指标 | before | after | Δ |
|---|---:|---:|---:|
| **进程 wall** | 49.58 s | **45.27 s** | **−4.31 s (−8.7%)** |
| egglog wall（3 pass） | 39.083 s | 35.122 s | −3.96 (−10.1%) |
| search + apply | 31.731 s | 28.175 s | −3.56 |
| merge（行提交） | 0.377 s | 0.328 s | −0.05 |
| rebuild | 5.660 s | 5.486 s | −0.17 |
| rule matches | 5,785,630 | 5,785,604 | −26 |
| extraction（Tiger） | 6.848 s | 6.584 s | −0.26 |
| serialization | 1.717 s | 1.582 s | −0.14 |
| **优化后 bril 输出** | sha256 `d18709571e6c…` | 同 | **完全一致** |

逐 pass（wall / search / tuples）：4.538→4.261 / 3.607→3.408 / 245,442→245,442；
11.999→11.105 / 10.054→9.232 / 649,252→649,268；22.546→19.756 / 18.070→15.535 / 697,997→697,944。
（第 1、2 pass 的库行数有 +16 / −53 的差异，约 0.008%，来自调度/timestamp 顺序；
最终抽出的程序逐字节相同。）

逐规则（三个 pass 合计）：

| rule | before | after |
|---|---:|---:|
| `Smaller` + `(= cost1 cost2) → union t1` | **2.311 s / 8,430 matches（274 µs/match）** | 删除 |
| `Smaller` + `(< cost1 cost2) → union t1` | 1.985 s / 79,664 | 被 `<=` 取代 |
| `Smaller` + `(<= cost1 cost2) → union t1` | — | 1.968 s / 88,068 |
| `PotentialExtractedExpr` + `(< cost 0) → panic` | **0.569 s / 0 matches** | 删除 |
| ruleset `terms-helpers-helpers` | 6.291 s（+2.050 rebuild） | 3.906 s（+1.967） |
| ruleset `terms-helpers` | 3.040 s | 2.396 s |

直接归因：`=` 规则 2.311 s + panic 0.569 s = **2.90 s**；其余 1.06 s 来自更少的 merge/rebuild
和更小的中间库（extraction/serialization 各降 0.26/0.14 s）。

## 为什么正好是这两处（对应"marker / 省 join"的判断）

- **panic 规则 = 纯搜索浪费**：0 命中的全表扫。这正是"局部信息预测"应该消灭的那类 match
  —— 它没有任何产出，只是把 6.9 万行过一遍。删掉即收益。
- **tie 规则 = 一次列间绑定 join**：`(= cost1 cost2)` 的 `=` 在两个 i64 列之间，引擎把它当
  *绑定/连接* 而不是过滤器，所以 274 µs/match，比 `<`/`>` 的 20–25 µs 贵十倍。
  折叠进 `<=` 后，`<=` 只花 1.968 s 就覆盖了原来 4.296 s 的 88K matches。

## 剩下的 6.3 s 不是"扫描浪费"，marker 在这里省不动

`Smaller` 的 `<`/`>`（3.89 s）+ improve 规则（2.38 s）加起来仍占 6.3 s。但它们：

- **match 数已接近最优**：183K ≈ 2.5 × 72,255（`Smaller` 节点数）。egglog 是 semi-naive，
  没有"每轮全表重扫"；这些 match 分布在 10,482 次 ruleset iteration 里。
- **贵在每次 match 的副作用**：每次 match 都做一次 `union` → congruence/rebuild，
  采样里 `ColumnIndex::rebuild_full` 占 self-time 13%、malloc/free 36%。
- **新信息是真的**：70,686 次 improve = 70,686 次最优值更新，每次都造一个 `Smaller` 节点
  等规则来解析。marker 无法消灭"新信息"，只能改变它的**表示**。

所以要让 marker 生效，需要下列之一（都不在这个 engine 里）：

1. **primitive merge**（声明级，非 rule 级）：`ExtractedExpr` 改成
   `:merge (min old new)` 之类，整块删掉 `Smaller` 与 3.9 s 解析。同一个生成程序里
   `Expr-size`/`LoopNumItersGuess` 已经是这个写法。
2. **引擎 dirty-set / `:on_merge`**：让 improve 规则只由"本 iteration 真正变化过的 key"驱动。
   `egglog/src/ast/mod.rs:663-676` 有 `:on_merge` 的文档，但**这个 engine 没有实现**
   （parser 里没有 `on_merge`），也没有 `count` 聚合，所以纯 rule 表达不出"局部标记"。

## 复现

```sh
cd /Users/mineralsteins/Repos/egg_related/check_statewalk
git apply /Users/mineralsteins/Repos/egg_related/egg_layout/experiments/rule_marker_search_savings/terms.egg.diff
git apply /Users/mineralsteins/Repos/egg_related/egg_layout/experiments/raytrace_phase_time/instrumented/dag_in_context_phase_report.patch
LLVM_SYS_180_PREFIX=/opt/homebrew/opt/llvm@18 \
LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/cbc/lib:/opt/homebrew/opt/coinutils/lib \
  cargo build --release --offline
EGGCC_PHASE_REPORT=1 EGGCC_PHASE_RULES=1 /usr/bin/time -l \
  ./target/release/eggcc --run-mode=optimize benchmarks/passing/raytrace/raytrace.rs \
  >/tmp/after.bril 2>results/after.log
```

文件：`terms.egg.diff`（rule-only 改动）、`results/before.log`（基线，含逐 rule 明细）、
`results/after.log`、`results/summary.json`。

`check_statewalk` 工作树在生成这些结果后已还原干净；改动只以 patch 形式保存在本目录。
基线来自 `experiments/raytrace_phase_time/`（同一二进制、同一 flags）。
单次运行，不做统计显著性声明；但 pass 级降幅（−6%/−7%/−12%）方向一致，且逐规则归因与预期完全吻合。

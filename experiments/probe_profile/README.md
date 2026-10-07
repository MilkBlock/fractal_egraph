# raytrace 的 search 为什么贵：一次 match 要筛 28 行候选

> **2026-10 修正。** 本文件第一版把 probe 数当成"每次 match 查了几次索引"，
> 再乘 ~1 µs 拟合出 18.9 s。**那个乘法是错的**——probe 是"准备"不是"工作量"，
> 量纲不对（见下"三个数先分清"）。新增 `probed_keys` / `candidate_rows` 两个计数器后，
> 真正被数出来的是：**整段 search 筛了 8,712 万行候选，才产出 309 万次规则触发。**
> 结论从"不是中间结果爆炸"改成了"**就是候选行太多**"。

## 一句话

**28 行候选换 1 次规则触发。** 每次触发 ≈ 5.28 µs，摊到每行候选 ≈ **187 ns**。
驱动键（索引的第一列）**区分度不够**——索引只能把 28 行缩到 1 行，
剩下的 27 行要靠后续约束一个个否掉。这正好是你说的"驱动键不够窄"。

## 三个数先分清（这也是你问的"probe 是什么"）

拿规则 `(rule ((= lhs (Smaller (TCPair t1 cost1) (TCPair t2 cost2))) (<= cost1 cost2)) ...)`
来举例，它的计划是 `INTERSECT TCPair(cost2) | Smaller(@c2)`：

| 名词 | 代码里的意思 | 量 | ÷ match |
|---|---|---:|---:|
| **match** | 规则左端**完整**命中一次 = 动作执行一次（`execute.rs:674` 的 `match_counter.read_matches(plan.actions())`）。这是**产出**：一次 union/set | **3,093,841** | 1 |
| **probe** | **建一次索引句柄**：`get_index` 调一次（`execute.rs:1222`）。**每个 stage 每个 scan 一次**，建好之后拿它去遍历 cover 的**所有**行。这是**准备**，不是查找次数 | 18,657,537 | 6.03 |
| **probed key** | 探针吐出的**一个不同键值**（`Prober::for_each` 回调一次）——"cost2 = 17 的那些行" | 32,161,076 | 10.40 |
| **candidate row** | 这些键值底下**挂的行数之和**。每一行都还要过剩下的约束（`refine_subset` / 列等式 / 内建谓词） | **87,119,445** | **28.2** |

**所以 probe ≠ match**，也不是"一次查找"。probe 与 match 之间隔着 10.4 个键、28.2 行候选。
一个 stage 扫 1 万行 cover、探 4 个索引，只记 **4** 次 probe；真正的 1 万×N 次行级查找
在 `Prober::for_each` / `Prober::get_subset` 里，才是 `candidate_rows` 数的东西。
第一版把 6 次"准备"当成 6 次"工作"去乘时间，纯属巧合地对上了数。

## 实测（`/tmp/rt.egg`，单进程、10,123 次迭代、380 条规则）

| 指标 | 数量 | 说明 |
|---|---:|---|
| **match**（动作执行次数） | **3,093,841** | 全进程累计 |
| search + apply | **16.34 s**（ruleset 口径 16.67 s） | 5.28 µs / match |
| **candidate row**（探测到的候选行） | **87,119,445** | **187 ns / 行**，28.2 行 / match |
| probed key（不同键值） | 32,161,076 | 10.4 键 / match |
| probe（索引句柄构建） | **18,657,537** | 6.03 / match |
| └ `sparse_column` | **13,643,271（73.1%）** | subset ≤ 8 行（`SMALL_RESIDUAL=8`），走"每次现建、不缓存"的栈上索引 |
| └ `dynamic_column` | 3,328,010（17.8%） | 单列、非 dense，用 per-trie-node 缓存 |
| └ `cached_column` | 1,686,046（9.0%） | dense 且 > 半表，用缓存列索引 |
| └ `dynamic_tuple`（`group_by_key`） | **180** | 多列探测**从不缓存**，但几乎没发生 |
| 缓存索引命中 / 重建 | 1,664,720 / **88,853** | 重建里只有 **232** 次是全量 |
| 索引**重建真正扫过的行** | **6,583,321** | 658 万行 → **小头** |
| congruence rebuild / merge | 3.83 s / 0.37 s | |

**口径 caveat**：这些计数器是**进程全局**的，量的是 `/tmp/rt.egg` 这个**导出脚本**的完整运行
（10,123 次迭代 / 16.3 s search），**不是** `experiments/raytrace_phase_time/` 里那个真实
eggcc 3-pass 跑（23,378 次迭代 / 31.7 s search）。两者不是同一个程序，别把绝对秒数对调。

## 这说明什么

1. **不是"微小探针 × 天文数字次"。** 73% 的 probe 确实打在 ≤8 行的 subset 上，
   但那是**准备阶段**的分布；真正的工作量是 87.1M 行候选，平均每个键下挂
   `87,119,445 / 32,161,076 = 2.7` 行，每个 match 前面躺着 28.2 行。
2. **索引刷新依然是小头**：真实增量 **658 万行**、其中仅 **232 次全量**。
   （第一版用 `table.all().size()` 记成 4.96 亿是上界，已改成 `subset.size()`。）
   失效机制很激进——`merge_all` 每轮把**全库**每张表的索引 catalog 都 reset
   （`free_join/mod.rs:748-761`）——但 `Index::refresh` 先比版本号、相同就 early return
   （`hash_index/mod.rs:66-95`），所以真重建量不大。**这条结论不受这次修正影响。**
3. **贵在"过滤比"。** 索引把 32.2M 个键缩到 87.1M 行，然后只留下 3.09M 个 match：
   每 28 行候选才活下来 1 行。这是"**驱动键太粗**"的signature：
   索引那一列（例子里是 `TCPair` 的 value 列 `cost2`）没能把行分开，
   后面的 `<=` / `Smaller` 约束只能逐行否掉。

## 这对"更窄的驱动键"/触点意味着什么

- **收益的正确度量是"候选行数下降"，不是"探针次数下降"。** 上一节那些窄 relation
  （73% 的 subset 本来就 ≤8 行）说明：**把表变窄不自动有用**，要把**同一键下的行数**变少。
- 端到端验证要用 `28.2 candidate rows/match` 这个数：改完规则后再跑一遍，
  如果这个数降下来了、search 秒数跟着降，才算真的有效。
- `__smallerCosts` 那次为什么单 pass 家族减半、端到端却亏：现在有了正确的数就能直接检查
  ——它的 144,446 次 match 花了多少 candidate rows，以及新表本身被 `merge` 弄脏的成本
  （rebuild 5.49→6.11 s、迭代 23,380→25,123）。**这是下一步要补的测量**，我不再凭 1 µs 猜。
- 你的**条件 1（触点不能靠重跑原来的大 join 才得到，要从产生它的 apply 直接发布）**
  仍然成立，而且现在的理由是实的：只有"发布时不再筛那 28 行"，
  才真的把 87.1M 行候选里的对应部分省掉。

## 复现与工具

计数器在本地 egglog 的 `core-relations/src/lib.rs` 的 `probe_counters`：
`get_index` 分支 6 处 + `Prober::for_each` 2 处 + `Prober::get_subset` 1 处
（`free_join/execute.rs`）、缓存访问 4 处（`free_join/mod.rs`）、`refresh` 1 处
（`hash_index/mod.rs`）。

```sh
cargo build --release --offline --bin egglog
EGGLOG_PROBE_COUNTS=1 egglog/target/release/egglog /tmp/rt.egg --report-level time-only
# match 总数要从报告里取：
egglog/target/release/egglog /tmp/rt.egg --report-level time-only --save-report /tmp/rt.json
```

**Caveat**：计数器是**无条件**的 relaxed atomic 自增（`candidate_rows` 那条约 3200 万次），
会给这次运行加可观开销；**墙钟不要当基准**（基准用 `experiments/eggcc_passes/raytrace/`），
**计数本身不受影响**。

## 下一步（有数据支撑的）

1. **per-table / per-rule 的候选行计数**：现在 87.1M 是全局的。挂到 `TCPair` / `Smaller` / `lsr-inv`
   上，就能直接指向"哪个键太粗"，而不是靠 6 条热规则的时间排序猜。
2. **重测 `__smallerCosts` 变体**，用 `candidate rows/match` 而不是 probe 数判断它到底省了什么。
3. **合成复现**：在 demo 里加第二条规则，让它在比较规则运行期间持续往 `CP` 插行，
   预期 candidate rows/match 会跳到 raytrace 量级；到那时再测触点才是有意义的。

---

## 附：索引"重建/失效"机制对应 egglog 的哪些代码

### A. 版本协议（表怎么表示自己变了）

| 代码 | 作用 |
|---|---|
| `TableVersion { major: Generation, minor: Offset }` — `core-relations/src/table_spec.rs:43-51` | `major` 变化**作废所有 RowId**；`minor` 只是"表变长了"的追加偏移 |
| `Table::version()` → `(self.generation, next_row_id())` — `table/mod.rs:518-523` | 每次访问时取当前版本 |
| `Table::updates_since(offset)` → 新增行的 dense range — `table/mod.rs:525-528` | 增量重建的输入 |
| `TableInfo.indexes: IndexCatalog<SmallVec<[ColumnId;4]>, HashIndex>`、`TableInfo.column_indexes: IndexCatalog<ColumnId, HashColumnIndex>`（两个成员都声明为 `Arc<ResettableOnceLock<Index<_>>>`）— `core-relations/src/free_join/mod.rs:138-139` | **缓存索引的仓库**：单列索引一张 catalog，复合（多列）索引另一张 |

### B. 失效（谁把它们弄脏）

| 代码 | 作用 |
|---|---|
| **`Database::merge_all` 结尾** — `core-relations/src/free_join/mod.rs:748-761`<br>注释原文：*"Reset all indexes to force an update on the next access"* | **每轮 merge 之后，把全库每张表的两个索引 catalog 全部 `reset()`**——不是只 reset 变了的表 |
| `Database::materialize_virtual_table` — `core-relations/src/free_join/mod.rs:948-950,954-958` | 虚拟行物化后同样 reset（且不宣告逻辑插入） |
| `Database::install_compact_cells` — `core-relations/src/free_join/mod.rs:1092-1106`（最后一行 `info.indexes=IndexCatalog::new(); info.column_indexes=IndexCatalog::new();`） | 直接换成新的空 catalog |
| `ResettableOnceLock` — `concurrency/src/resettable_oncelock.rs:20`（`get` :36、`get_or_update` :46、`reset` :66） | `get()` 只在 completed 时返回；`reset()` 把 `Once` 复位；`get_or_update` 只在未 completed 时跑更新闭包 |

调用频率：`merge_all` 每个 ruleset run 调用一次（`core-relations/src/free_join/execute.rs:683`，计时在 :682-684，就是 `RuleSetReport.merge_time` 计的那一段）。
raytrace pass 3 有 10,123 次迭代 × 261 张表 → 数百万次 `reset`（单独很便宜，我实测 `merge_time` 合计仅 0.33 s/3 pass）。

### C. 重建（谁真正干活）

| 代码 | 作用 |
|---|---|
| `get_index_from_tableinfo`（复合键）— `core-relations/src/free_join/mod.rs:986-1008`；`get_column_index_from_tableinfo`（单列）— `:1013-1035` | 懒创建 + `index.get_or_update(\|index\| index.refresh(table))` |
| **`Index::refresh`** — `core-relations/src/hash_index/mod.rs:66-95`（`needs_refresh` 在 :62-64，版本写回在 :94） | ① 版本号相同 → **early return**（这就是为什么"全库 reset"没炸）；② `major` 不同 → `table.clear()` + `rebuild_full`；③ 否则 `updates_since` → `refresh_serial`；④ `subset.size() > 20_000` 且 rayon 线程 >1 走 `merge_parallel`（`core-relations/src/parallel_heuristics.rs:14`） |
| 触发点：探针路径的 `Cached` / `CachedColumn` 分支 — `core-relations/src/free_join/execute.rs:1252-1270` | 只有"subset 是 dense 且大于半张表"时才走这条缓存路径 |
| `DynamicColumn` 用的 per-trie-node 缓存 — `execute.rs:1279` → `trie_node.get_cached_index(cols[0], info)`，实现是 `execute.rs:770-782`（`subset_trie_node` 上的 `OnceLock`，**独立于**上面那两张 catalog） | 这条路**不**经过上面那两张 catalog |
| `SortedWritesTable` 自己的 `rebuild_index: Index<ColumnIndex>` — 字段 `table/mod.rs:215`，刷新在 `table/rebuild.rs:60-69`（`refresh_rebuild_index`） | 表内部用于「哪些行需要重写」的索引，**和上面那两张 catalog 无关** |

### D. 别和 "congruence rebuild" 混了

codex 报告里那 6.18 s 的 **Rebuild** 不是索引刷新，而是**同余闭包重建**：

| 代码 | 作用 |
|---|---|
| `egglog-bridge/src/lib.rs:984`（`incremental_rebuild_rules`）/ `:997`（`incremental_rebuild_rule`，规则名 `` `incremental rebuild {table:?}, {col:?}` `` 在 :1005，并显式 `set_plan_strategy(PlanStrategy::MinCover)` 在 :1008）、`:1046`（`nonincremental_rebuild`） | 为每张表生成**内部规则**来维护 `$rebuild` 表 |
| `egglog-bridge/src/lib.rs:774`（`fn rebuild`）/ `:902`（`rebuild_parallel`）；`rebuild_timer` 在 :756，写回 `iteration_report.rebuild_time` 在 :765 | 报告里 rebuild 时间的来源 |
| `Database::apply_rebuild` — `core-relations/src/free_join/mod.rs:542`；实现 `core-relations/src/table/rebuild.rs`（`do_rebuild` :71、`rebuild_incremental` :142、`rebuild_nonincremental` :219） | 把 union 后的新代表写回各行 |

两套东西的失效/重建触发条件完全不同：**索引刷新**由"表版本变了 + 有人要 probe 那列"触发（我的计数器量的是这个，实测 658 万行，小头）；**同余重建**由"union 发生后 rebuild 规则跑一轮"触发（codex 量到 6.18 s，是第二大头）。

### E. 成本归因（修正版）

- **match** 3,093,841 次：规则动作执行次数（`match_counter.read_matches`）。
- **probe** 18,657,537 次：索引句柄构建次数，每 stage 每 scan 一次。
- **candidate row** **87,119,445** 行：探针吐出的键底下挂的行数之和，每行都要过后续约束。
  → **28.2 行候选 / 1 次 match**，search 16.34 s ⇒ **187 ns / 行候选**。
- 索引真实重建只有 **658 万行 / 232 次全量** → 小头。

**归因**：贵在**过滤比 28:1**（索引把 32.2M 键缩成 87.1M 行，最终只活 3.09M 个 match），
即**驱动键区分度不够**；不在索引重建，也不在"探针次数"本身
（probe 是准备动作，不能直接乘时间——第一版那样算是错的）。

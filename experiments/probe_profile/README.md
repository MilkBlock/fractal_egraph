# 为什么 raytrace 的 search 那么贵：probe 计数（而不是搜索空间爆炸）

你指出我的合成 demo"图不够 sparse"，这个批评是对的。我用**探针路径计数器**在真实 raytrace 上
量了一下，找到了真实原因：**不是中间结果爆炸，而是海量微小探针 + 索引刷新抖动**。

## 实测（raytrace pass 3，`/tmp/rt.egg`，380 条规则、10,123 次迭代）

| 指标 | 数量 | 说明 |
|---|---:|---|
| 探针总数 | **18,657,537** | 一次 search 里 1866 万次 probe |
| └ `sparse_column` | **13,643,271（73.1%）** | **subset ≤ 8 行**（`SMALL_RESIDUAL=8`），走"每次现建、不缓存"的栈上索引 |
| └ `dynamic_column` | 3,328,010（17.8%） | 单列、非 dense，用 per-trie-node 缓存 |
| └ `cached_column` | 1,686,046（9.0%） | dense 且 > 半表，用缓存列索引 |
| └ `dynamic_tuple`（`group_by_key`） | **180** | 多列探测**从不缓存**，但这里几乎没发生 |
| 缓存索引命中 | 1,664,720 | |
| 缓存索引**重建** | **88,853** | 其中只有 **232** 次是全量重建，其余是增量 |
| **重建时真正扫过的行数**（增量 delta） | **6,583,321** | 658 万行；见下面的修正 |
| ~~上界~~（误用 `table.all()`） | ~~495,913,202~~ | **这个数字是错的，已修正** |

换算：每个 match 平均 **3.22 次探针**；**每次探针平均对应 26.6 行被重新索引**。

## 这说明什么

1. **join 不是在算大中间结果**——恰恰相反：**73% 的探针打在 ≤8 行的 subset 上**。
   图是 sparse 的（717K 节点 / 544K e-class = 1.32 节点/类，几乎没有共享），
   所以每次探测都很窄，但**次数是天文数字**。
2. **索引刷新是小头**：真实增量只有 **658 万行**、其中仅 **232 次全量重建**。
   （我第一版用 `table.all().size()` 记成了 4.96 亿，那是**上界**，已修正为 `subset.size()`。）
   失效机制本身很激进——`merge_all` 每轮**把全库所有表的索引 reset**
   （`free_join/mod.rs:748-756`）——但因为 `Index::refresh` 会先比版本号、相同就 early return
   （`hash_index/mod.rs:66-70`），真正的重建量并不大。
3. 所以"search 占 78.7%"的物理含义是：**每次 match 要付 5.76 次窄探针**——
   3.24M matches × 5.76 probes/match × ~1 µs/probe ≈ **18.9 s**，正好等于 pass 3 的 search 时间。
   即成本 = **探针次数 × 每次探针的开销**，而不是"枚举了大量候选组合"，也主要不是索引重建。

## 因此我的合成 demo 为什么测不出效果（诚实的解释）

| | 探针 | 缓存索引重建 | 重建扫过的行 | 每 match |
|---|---:|---:|---:|---:|
| raytrace pass 3 | 18,657,537 | 88,853（锁被 reset 的次数） | 6,583,321 | 17.2 µs |
| 我的合成（K=M=30,000） | 127,916 | 4 | ~0 | 1.24 µs |

两个 demo 的**分支占比其实一样**（都是 `sparse_column` 主导：73% vs 85%），
但**重建次数差 22,000 倍**。我的合成只有一条规则、**没有任何并发插入**，
所以被探测表的索引一次建好就一直新鲜；raytrace 里索引一直在被别的规则弄脏。
**这才是 13.8× 每-match 差异的来源，也正是"图够不够 sparse"背后的真机制。**

## 对触点/白名单那条路的修正

- **触点的收益必须体现为"减少探针次数"或"让探针打在不会被弄脏的表上"**，
  而不是"让 relation 更窄"。窄本身在这个负载上不稀奇（73% 的探针本来就是 ≤8 行）。
- 这解释了 `__smallerCosts` 那次为什么单 pass 家族减半、端到端却亏：
  **builder 是用同样的探针去发布的**（144,446 次 match，8.7 µs/match），
  省下的探针又被它自己花了；再加上新表本身也被 `merge` 弄脏，rebuild 反而涨了（5.49→6.11s）
  、迭代数涨了（23,380→25,123）。
- 因此你的**条件 1（触点不能靠重跑原来的大 join 才得到，要从产生它的 apply 直接发布）**
  不是风格问题，而是**收益的充要条件**：只有"发布时不再探一遍"，
  才能把 88,853 次重建 / 4.96 亿行的那部分省下来。
  而"从产生它的 apply 直接发布"在 `Smaller` 这个例子上又卡在
  **merge 表达式无法解构 `TermAndCost`**（所以需要我说的那个原语）。

## 复现与工具

计数器已加入本地 egglog（`core-relations/src/lib.rs` 的 `probe_counters`，4 处 `bump`
在 `free_join/execute.rs` 的 `get_index` 分支、2 处在 `free_join/mod.rs` 的缓存访问、
1 处在 `hash_index/mod.rs` 的 `refresh`）：

```sh
cargo build --release --offline --bin egglog
EGGLOG_PROBE_COUNTS=1 egglog/target/release/egglog /tmp/rt.egg --report-level time-only
```

**Caveat**：计数器是**无条件**的 relaxed atomic 自增（约 1900 万次），本身会给这次运行加
几十毫秒开销；所以这一轮的墙钟不要当基准，基准用 `experiments/eggcc_passes/raytrace/`。
计数本身不受影响。

## 下一步（有数据支撑的）

1. **先把"重建成本"归因到表**：现在的计数是全局的，加一个 per-table 的重建行数计数，
   就能直接指出是 `TCPair` 还是 `Smaller` 还是 `lsr-inv` 在反复重建。
2. **合成复现**：在 demo 里加第二条规则，让它在比较规则运行期间**持续往 `CP` 插行**
   （即制造索引失效），预期每-match 成本会跳到 raytrace 量级；到那时再测触点才是有意义的。
3. **真正的优化方向**（比触点更直接）：减少"探测表的失效频率"——例如把插入批量提交后再刷新、
   或者让高频探测的列走**不依赖缓存索引**的路径（sparse 路径本来就是），
   或者把热数据搬到**运行期只写一次**的表里（这正是 Cell/白名单在原理上能帮上忙的地方）。

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
| `get_index_from_tableinfo`（复合键）— `core-relations/src/free_join/mod.rs:986-1011`；`get_column_index_from_tableinfo`（单列）— `:1013-1029` | 懒创建 + `index.get_or_update(\|index\| index.refresh(table))` |
| **`Index::refresh`** — `core-relations/src/hash_index/mod.rs:66-92`（`needs_refresh` 在 :62-64） | ① 版本号相同 → **early return**（这就是为什么"全库 reset"没炸）；② `major` 不同 → `table.clear()` + `rebuild_full`；③ 否则 `updates_since` → `refresh_serial`；④ `subset.size() > 20_000` 且 rayon 线程 >1 走 `merge_parallel`（`core-relations/src/parallel_heuristics.rs:14`） |
| 触发点：探针路径的 `Cached` / `CachedColumn` 分支 — `core-relations/src/free_join/execute.rs:1252-1270` | 只有"subset 是 dense 且大于半张表"时才走这条缓存路径 |
| `DynamicColumn` 用的 per-trie-node 缓存 — `execute.rs:1279` → `trie_node.get_cached_index(cols[0], info)`，实现是 `execute.rs:770-783`（`subset_trie_node` 上的 `OnceLock`，**独立于**上面那两张 catalog） | 这条路**不**经过上面那两张 catalog |
| `SortedWritesTable` 自己的 `rebuild_index: Index<ColumnIndex>` — 字段 `table/mod.rs:215`，刷新在 `table/rebuild.rs:60-69`（`refresh_rebuild_index`） | 表内部用于「哪些行需要重写」的索引，**和上面那两张 catalog 无关** |

### D. 别和 "congruence rebuild" 混了

codex 报告里那 6.18 s 的 **Rebuild** 不是索引刷新，而是**同余闭包重建**：

| 代码 | 作用 |
|---|---|
| `egglog-bridge/src/lib.rs:984`（`incremental_rebuild_rules`）/ `:997`（`incremental_rebuild_rule`，规则名 `` `incremental rebuild {table:?}, {col:?}` `` 在 :1005，并显式 `set_plan_strategy(PlanStrategy::MinCover)` 在 :1008）、`:1046`（`nonincremental_rebuild`） | 为每张表生成**内部规则**来维护 `$rebuild` 表 |
| `egglog-bridge/src/lib.rs:774`（`fn rebuild`）/ `:902`（`rebuild_parallel`）；`rebuild_timer` 在 :756，写回 `iteration_report.rebuild_time` 在 :765 | 报告里 rebuild 时间的来源 |
| `Database::apply_rebuild` — `core-relations/src/free_join/mod.rs:542`；实现 `core-relations/src/table/rebuild.rs`（`do_rebuild` :71、`rebuild_incremental` :142、`rebuild_nonincremental` :219） | 把 union 后的新代表写回各行 |

两套东西的失效/重建触发条件完全不同：**索引刷新**由"表版本变了 + 有人要 probe 那列"触发（我的计数器量的是这个，实测 658 万行，小头）；**同余重建**由"union 发生后 rebuild 规则跑一轮"触发（codex 量到 6.18 s，是第二大头）。

### E. 修正后的成本归因（pass 3，search 18.9 s）

- 探针 **18,657,537** 次（73% 打在 ≤8 行 subset 上，走 `SparseColumnIndex::new` 每次现建、不缓存）
- 索引真实重建 **658 万行**、**232 次**全量 → 小头
- 3.24M matches × **5.76 probes/match** × ~1 µs ≈ 18.9 s → **大头是探针次数本身**

所以要优化的是**探针路径**（`get_index` 的分支选择 + `SparseColumnIndex::new` + 每次 probe 的约束/绑定处理），
而不是索引重建策略。

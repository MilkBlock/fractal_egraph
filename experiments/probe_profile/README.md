# raytrace 的 search 为什么贵：1866 万次微小探针，每次只碰 4.7 行

> **修正记录（两次）。**
> 1) 第一版把 probe 数当成"每次 match 查了几次索引"，乘 ~1 µs 拟合出 18.9 s；**量纲错了**，
>    probe 是"建索引句柄"，每 stage 每 scan 一次。
> 2) 第二版据此说"28 行候选换 1 次触发 ⇒ 驱动键太粗"。加了 `probed_keys` /
>    `candidate_rows` 的**拆分**后，**这条也不成立**：枚举时**每个键底下只有 1.14 行**，
>    键几乎是唯一的，**根本没有"键太粗"这回事**。

## 一句话

一次 match 要付 **6.03 次探针**，每次探针只碰 **4.7 行**（其中 1.14 行/键）。
所以 28.2 行/match 不是"一个键下面挂了 28 行"，而是 **~6 个 stage × 每个 4.7 行** 累出来的。
**join 又长又碎，不是又宽又粗。**

## 三个数先分清

拿 `(rule ((= lhs (Smaller (TCPair t1 cost1) (TCPair t2 cost2))) (<= cost1 cost2)) ...)`
举例（计划：`INTERSECT TCPair(cost2) | Smaller(@c2)` → `DRIVE TCPair[t1@c0,cost1@c1]` → …）：

| 名词 | 代码里的意思 | 量 | ÷ match |
|---|---|---:|---:|
| **match** | 左端完整命中一次 = 动作执行一次（`execute.rs:674` `match_counter.read_matches`） | **3,093,841** | 1 |
| **probe** | **建一次索引句柄**：`get_index` 调一次（`execute.rs:1222`），**每 stage 每 scan 一次** | 18,657,537 | 6.03 |
| **probed key** | **枚举时**吐出的一个不同键值（`Prober::for_each` 回调一次） | 32,161,076 | 10.40 |
| **candidate row** | 探针吐出的 subset 里的行数之和（枚举 + 单键查找两条路都算） | **87,119,445** | **28.16** |

注意 **probe / probed key / candidate row 不是一条漏斗上的三级**，因为 `candidate row`
由**两条独立的路**喂出来，而只有第一条路的键被单独计数：

| 路径 | 计数 | 量 | ÷ match | 每次产出 |
|---|---|---:|---:|---|
| ① 整索引枚举 `Prober::for_each` | `probed_keys` → `enum_rows` | 36,695,340 | 11.86 | **1.14 行/键** |
| ② 单键查找 `Prober::get_subset` | `lookup_calls` → `lookup_rows` | 50,424,105 | 16.30 | **1.92 行/次** |
| | 合计 | 87,119,445 | 28.16 | |

**所以"为什么 28.16 > 10.40 > 6.03"的算术答案就是**：
`28.16 = 11.86 (枚举) + 16.30 (查找)`，而**查找这条路的 16.30 行完全不进 `probed_keys`，
它的调用次数也不进 `probes`**——一个 prober（1 次 probe）建好之后可以被复用做多次
`get_subset`，那些行照样要过后续约束，所以照样计入 `candidate_rows`。
比值 `lookup_calls/probe = 1.41`、`probed_keys/probe = 1.72`：**一次探针平均只枚举 1.7 个键、
只查 1.4 次**，也就是说探针背后的 subset 小到平均只有几行——这正是"73% 的探针打在 ≤8 行"。

## 实测（`/tmp/rt.egg`，单进程、10,123 次迭代、380 条规则、3,093,841 次 match）

| 指标 | 数量 | ÷ match | 说明 |
|---|---:|---:|---|
| search + apply | 16.34 s | 5.28 µs | ruleset 口径 16.67 s |
| **probe**（索引句柄构建） | **18,657,537** | 6.03 | 打平：**876 ns / probe** |
| └ `sparse_column` | 13,643,271（73.1%） | 4.41 | subset ≤ 8 行（`SMALL_RESIDUAL=8`），每次现建不缓存 |
| └ `dynamic_column` | 3,328,010（17.8%） | 1.08 | 单列、非 dense，per-trie-node 缓存 |
| └ `cached_column` | 1,686,046（9.0%） | 0.54 | dense 且 > 半表，缓存列索引 |
| └ `dynamic_tuple` | 180 | 0.00 | 多列探测从不缓存，但几乎没发生 |
| **probed key**（枚举出的键） | 32,161,076 | 10.40 | |
| **candidate row**（碰到的行） | **87,119,445** | **28.16** | 打平：**187 ns / 行** |
| └ 枚举来的 | 36,695,340 | 11.86 | **1.14 行 / 键** ← 键几乎唯一 |
| └ 查找来的 | 50,424,105 | 16.30 | **1.92 行 / 次**（26,249,838 次调用） |
| 索引重建真正扫过的行 | 6,583,321 | 2.13 | 232 次全量 → **小头** |
| congruence rebuild / merge | 3.83 s / 0.37 s | | |

**口径 caveat**：计数器是**进程全局**的，量的是 `/tmp/rt.egg` 这个**导出脚本**的完整运行
（10,123 次迭代 / 16.3 s search），**不是** `experiments/raytrace_phase_time/` 里那个真实
eggcc 3-pass 跑（23,378 次迭代 / 31.7 s search）。两者不是同一个程序，别把绝对秒数对调。

## 这说明什么

1. **没有"驱动键太粗"。** 枚举路径 **1.14 行/键**、查找路径 **1.92 行/次**，
   探针打的基本是唯一/近唯一的列（例如 `TCPair` 的 value 列是 63,071 distinct / 63,071 行）。
   上一版"索引只能把 28 行缩到 1 行"是我把两个路径的行数加在一起误读的，**撤回**。
2. **也没有冗余重扫。** 6.03 探针/match，而规则左端本来就有 4~6 个原子、每个原子至少要一次
   index access——**计划在探针数上接近下界**，没有明显的重复枚举。
3. **贵在"次数 × 每次固定开销"。** 18.66M 次探针，**平均每次只碰 4.7 行**；
   16.34 s / 18.66M ≈ **876 ns/probe**，而每行候选只要 ~187 ns。
   这两个数**只能说"都可能是大头"**：如果每次探针的固定开销（`SparseColumnIndex::new`
   扫 ≤8 行并排序、分支选择、`Prober` 组装）是 700 ns 量级，那它就是主因；
   如果只有 100 ns，那 87M 行的逐行工作是主因。
   **现有的计数器分不出这两种情况**——要在 `get_index` 内部和行循环内部各加计时才能定论。

## 对"更窄的驱动键"那条路的含义（第二次修正）

- "把驱动键变窄"**不再有依据**：键已经几乎是唯一的了，再窄无处可窄。
  上一轮我拿 28.2 当证据去支持它，是错的。
- 真正对应的量是 **`probe/match` 和 `ns/probe`**：
  - 要么**减少探针次数**（更少的 stage 进入次数）——比如减少规则里的原子数、
    或让多个原子共享一次索引遍历；
  - 要么**降低每次探针的固定开销**（`SparseColumnIndex::new` 这条 73% 的路径
    每次现建、不缓存，是最可疑的一处）。
- 触点/白名单那条路要拿这两个数重新论证：省掉的必须是**探针次数**，或者让探针落在
  **运行期只写一次**的表上（不会被 `merge` 反复 reset）。
  上一版写的"条件 1 是收益的充要条件"这个判断本身仍成立，但它的依据从"87M 行候选"改成
  "18.66M 次探针里省下多少"。

## 复现与工具

计数器在本地 egglog 的 `core-relations/src/lib.rs` 的 `probe_counters`：
`get_index` 分支 6 处 + `Prober::for_each` 3 处 + `Prober::get_subset` 4 处
（`free_join/execute.rs`）、缓存访问 4 处（`free_join/mod.rs`）、`refresh` 1 处
（`hash_index/mod.rs`）。

```sh
cargo build --release --offline --bin egglog
EGGLOG_PROBE_COUNTS=1 egglog/target/release/egglog /tmp/rt.egg --report-level time-only
# match 总数要从报告里取：
egglog/target/release/egglog /tmp/rt.egg --report-level time-only --save-report /tmp/rt.json
```

**Caveat**：计数器是**无条件**的 relaxed atomic 自增（`probed_keys` + `candidate_rows`
合计约 1.2 亿次），会给这次运行加可观开销；**墙钟不要当基准**（基准用
`experiments/eggcc_passes/raytrace/`），**计数本身不受影响**。

## 下一步（有数据支撑的）

1. **先分清 876 ns/probe 和 187 ns/row 谁是大头**：在 `get_index` 内部
   （分支选择 + `SparseColumnIndex::new`）和行循环内部（`refine_subset`、`get_cached_trie_node`）
   各加一个计时累加器。**这是唯一能决定下一步方向的测量**，不做它的话上面两个方向都是猜。
2. **per-table / per-rule 拆分**：现在 87.1M 行、18.66M 探针都是全局的。
3. **合成复现**：在 demo 里加第二条规则，让它持续往被探测的表插行，
   看 `ns/probe` 会不会跳到 raytrace 量级。

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

### E. 成本归因（第二次修正版）

| 量 | 值 | ÷ match | 每次 |
|---|---:|---:|---:|
| match（动作执行） | 3,093,841 | 1 | |
| probe（索引句柄构建） | 18,657,537 | 6.03 | **876 ns / probe** |
| └ 枚举路径 `for_each` | 32,161,076 键 | 10.40 | **1.14 行 / 键** |
| └ 查找路径 `get_subset` | 26,249,838 次 | 8.48 | **1.92 行 / 次** |
| candidate row（合计） | **87,119,445** | **28.16** | **187 ns / 行** |
| 索引重建扫过的行 | 6,583,321 | 2.13 | 小头（232 次全量） |

**归因**：`candidate_rows = enum_rows + lookup_rows`，两条路独立；
**探针背后的 subset 平均只有几行**（1.14 行/键、1.92 行/次），
所以 28.16 行/match 是 **~6 次探针 × 每次 ~4.7 行** 累出来的，**不是"键太粗"**。
键的选择性很好（`TCPair.value` 63,071 distinct / 63,071 行 = 1.0 行/键）。
探针数 6.03/match 也接近"每个原子一次访问"的下界，**计划没有冗余重扫**。

**尚未定论**：16.34 s 里 876 ns/probe（固定开销）与 187 ns/row（逐行工作）谁是主因，
现有计数器分不出来，需要在 `get_index` 内部和行循环内部各加计时。

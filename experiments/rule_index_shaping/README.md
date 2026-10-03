# 规则级 "index shaping"：机制已定位，但我的改写工具还没跑通

目标：**只改 `.egg` 里的 rule**，通过改变规则形态去影响 egglog 内部建哪些表索引，
从而省掉"再扫一遍 producer 表"。不新增任何索引数据结构。

- **状态：未完成。** 机制查清了，观察器接好了，但改写器还有 3 个我自己的 bug 没修完，
  所以**没有拿到任何 before/after 的搜索时间对比**。下面区分"已确认"和"未完成"。

## 已确认（来自源码 + 实测，可以据此决策）

### 1. 引擎能用的索引规则很窄

`free_join/execute.rs:1217-1263`，probe 一个 `(table, column)` 时：

| 分支 | 条件 | 结果 |
|---|---|---|
| `SparseColumn` | `subset.size() <= SMALL_RESIDUAL` 且 1 列 | 临时索引，不缓存 |
| `CachedColumn` | subset 是 Dense 且 `> 半个表`，1 列 | **用缓存的单列索引** |
| `Cached`（复合） | 同上，多列 | 用缓存的复合索引 |
| `DynamicColumn` | 否则，1 列 | subset 级临时索引（per trie node 缓存） |
| `Dynamic` | 否则，多列 | `group_by_key`，**每次重建，不缓存** |

结论：**只有在子集又大又密时才走缓存索引**；对 join 实际probe 的窄 residual，
单列索引是按 subset 现建的，复合索引甚至是每次重建。所以"物化一个索引"这件事，
在窄 probe 上本来就不是引擎的行为——**这不是改 rule 能直接决定的**。

### 2. 缓存到的索引是按**列号**存的，列号由声明顺序决定

`table_info.column_indexes.get_or_insert(col, …)`（`free_join/mod.rs:1008-1025`）以
`ColumnId` 为键、`table_info.indexes` 以 `SmallVec<[ColumnId;4]>` 为键。
ColumnId **就是该表 schema 里的位置**（0 = 第一列）。所以：

- 一个 constructor 的**声明顺序**（从而每列的位置）决定引擎可能缓存哪些单列索引；
- 只有被绑定到**第 0 列**的变量才可能命中 `CachedColumn`。

### 3. 实测：一次真实 eggcc pass 里，引擎**一个索引都没物化**

用只读观察器（下面）在 base 变体上测得：

```json
{"column_index_count": 0, "tuple_index_count": 0, "tables": []}
```

`applications=2777`、`logical_matches=4664`。因为 probe 的 subset 都很小/很窄，
全部落在 `SparseColumn` / `DynamicColumn` / `Dynamic` 分支。
**这直接说明"规则级 index shaping"在 eggcc 上大概率拿不到收益**：引擎本来就没有
可以"引导"的缓存索引；要改变的就是那个 `<= 半个表` 的启发式本身，那是引擎改动。

### 4. 观察器（本次唯一落地的代码）

为了不靠推断，加了一个**只读**自省钩子（不创建索引）：

- `core-relations/src/hash_index/mod.rs`：`IndexCatalog::keys()`（只读列出已存在的索引键）
- `core-relations/src/free_join/mod.rs`：`Database::materialized_indexes()`
- `egglog-bridge/src/lib.rs`：`EGraph::materialized_indexes()` 转发
- `egglog/src/lib.rs`：`EGraph::materialized_indexes()` 转发
- `src/native_analyze.rs`：`EGG_LAYOUT_INDEX_REPORT=1` 时写 `indexes.json`
- `src/native_saturated_rule_composition_pipeline.rs`：`Pipeline::engine_indexes()` 汇总各 ripen session

**注意**：这些改动**尚未提交**，且 `egglog/` 下本来就有大量先前未提交的
dirty 改动（我的 diff 与它们混在工作树里）。要提交需要先把我的部分单独分离出来。

## 未完成（我的 bug，不是机制问题）

`rewrite.py` 生成了 base / rot / row 三个变体，base 与源文件逐字节相同，但：

- `rot`（把捕获变量转到第 0 列）：**0 条规则被改写**。直接手验发现：被判定的候选
  `(TypeList-ith (TCons hd tl) 0)` 里第一个参数是嵌套调用而不是纯列，
  被我的"pure column"守卫挡掉了；而真正纯列的候选我没筛出来。
- `row`（包一层单列 helper 关系）：194 条规则被改写、35 个 helper，但生成的文件
  **有 parse error**（我第一版的插入位置把 `(= v (Fact …) (Rel …))` 拼错了，
  修了一处仍有问题）。
- 沿途踩过的坑（已记录，避免重犯）：
  1. 用 `(panic 'msg')` 的裸 fact 被旋转 → 字符串字面量跑到算符位置，parse error；
  2. 把 `(TCons hd tl)` 这种嵌套调用旋转 → 跨表表达式被塞进字面量列，type error；
  3. 把"嵌套调用"计入变量数 → 误判候选；
  4. `used_first` 去重集在规则间共享 → 第一条规则之后全部被挡。

所以 **base 之外的对比数据一个都没有**，本文不给任何"省了多少"的数字。

## 建议

1. **先做完 §"已确认 3"的判定**：如果引擎在 eggcc 上根本不物化索引，
   "规则级 index shaping"就没有作用点（结论 3 已经强烈指向这一点：0 个索引）。
   要做的是引擎侧的启发式（什么时候把 `Dynamic`/`DynamicColumn` 提升为 `Cached`），
   而不是 rule 改写。这跟我之前量的"eggcc 没有重复 join 可缓存（0%）、
   但有 92.3% 的规则级重复"是一致的：**缺的是更窄的驱动键，而驱动键由列序决定**。
2. 如果仍想走 rule 路线，**最小可行实验**是改 constructor 的**列顺序**
   （而不是改规则）：把多列 fact 里最常作为探针的那一列放到第 0 位。
   这同样只动 `.egg`，而且直接对应 `ColumnId` 的语义；但前提仍是引擎会缓存那个索引。
3. 或者干脆承认：这条路上**能省的只有"同一条规则被反复触发"这份重复**，
   而那需要"规则驱动的 join 顺序"（子句重排 / 驱动表选择），
   在 egglog 里表现为**把最窄的 fact 写在 body 最前面**——这是可以纯 rule 做的，
   但效果取决于 plan 是否尊重书写顺序（需要单独验证 `Query::build_cached_plan`）。

## 你的提议：派生更窄的驱动键 + `:unextractable`（机制上成立）

结论：**做法可行，而且我已经实测到引擎真的会为派生键建单列索引**。
这比我上面那条"rot/row 改写"的路子干净得多，因为 `unextractable` 消除了我踩的类型/解析坑。

### 为什么 `:unextractable` 是关键

`unextractable` **只影响 extraction**（`src/extract.rs:148-150`「constructor marked as
unextractable will not be used during extraction」；parser 见 `src/ast/mod.rs:620-634`）。
它**不影响规则的匹配/连接语义**。所以你提议的形态没有任何类型或语义代价：

```egglog
(sort <关键实体>Sort)
(constructor <KeyRel> (<列类型>...) <关键实体>Sort :unextractable)

(rule ((= e (Original a b)) (= k (Project e i)))    ; 从已有事实投影
      ((KeyRel k)) :ruleset build)                  ; 物化一把窄键

(rule ((KeyRel k) (Other o ...))                    ; 用窄键驱动原来的 join
      ((...)) :ruleset probe)
```

### 为什么它真的更窄：planner 按**表规模**选驱动表

`free_join/plan.rs:1365-1401` 的 `plan_free_join`：

```rust
size_info.push((atom, *size));      // 每个 atom 的当前规模
...
size_info.sort_by_key(|(_, size)| *size);   // 按规模升序
```

**驱动表 = 最小的那个 atom**，然后依次 probe 其余。所以：

- 原来的 `(Pair a b)` 若被 join 在**第 2 列**上，probe 的是 `Pair` 表的第 2 列；
- 投影出的 `(Pair-2nd b)` 是**单列表**，规模小得多，
  于是 planner 会选它当 cover，probe 的是**第 0 列**（`plan.rs:1455` 的 `cover_col`）。

### 实测（`results/derived-key.egg` + 观察器）

```egglog
(rule ((= p (Pair a b)) (= q (Pair a b))) ((Tag p q)) :ruleset probe)   ; 原形：join 在第 2 列
(relation Pair-2nd (Math))
(rule ((= p (Pair a b))) ((Pair-2nd b)) :ruleset build)                 ; 派生窄键
(rule ((Pair-2nd b)) ((Tag (Pair (V 0) b) (Pair (V 1) b))) :ruleset probe2)
```

引擎实际物化的索引：

```json
{"column_index_count": 4, "tuple_index_count": 0, "tables": [
  {"table": "Pair",     "column_indexes": [[4]]},
  {"table": "Pair-2nd", "column_indexes": [[3]]},
  {"table": "Tag",      "column_indexes": [[4]]},
  {"table": "V",        "column_indexes": []}
]}
```

对比 base（真实 eggcc pass）：`column_index_count = 0`。
**派生键把"零索引"变成了"派生表第 0 列有索引"**，正是你要的形态。

### 但有两个必须验证的前提（我还没有数据）

1. **`ColumnId` 是含时间戳的，所以列号是 4 / 3 而不是 1 / 0。**
   `Pair` 的列 4 = 用户第 2 列，`Pair-2nd` 的列 3 = 用户第 0 列。
   因此"窄键"必须真的落在**用户第 0 列**才会命中 `CachedColumn` 那条路径
   （`execute.rs:1252`），否则仍走 dynamic。派生表是单列的，天然满足。
2. **收益是"探针变窄 + 命中缓存索引"，不是"少建一张表"。**
   我没有量到实际搜索时间的下降。上面的证据只证明"索引被物化了、驱动表换了"，
   **不证明更快**。要证明必须跑对照计时。

### 对 eggcc 的注意点

在 eggcc 上落地时，派生键的价值取决于它是否**真的比原表小**。
eggcc 的热规则里 `probe` 列往往不是"窄"列：

- `(Get expr i)` 的 `i` 常常是字面量（`0`、`1`），派生出的一列表会很小 → 有希望；
- `PotentialExtractedExpr expr` / `ContextOf e ctx` 的列与主表**一一对应**，
  派生表规模 ≈ 原表 → **没有收益**，只是多一张表。

所以不能对所有规则无脑加键；应当先用 `observed-transitions.json` 里
每列的**取值去重率**挑出真正窄的列（这是我现在能立刻算的）。

## 建议（按此提议更新）

1. **先挑列**：对真实 trace 统计每条热规则各 reader slot 的取值重复率，
   只给"去重后远小于行数"的列建派生键。这一步纯分析，我今天就能出数。
2. **再验驱动表**：用 `EGG_LAYOUT_INDEX_REPORT=1` 确认派生键真的被选中当 cover
   （如上，观察器已经能做这件事）。
3. **最后计时**：同程序 `base` vs `derived-key` 两个变体跑 raytrace 的 phase report，
   看 `Smaller`/`Expr-size` 那几条的 µs/match 是否下降。
   如果第 1 步筛出来没有足够窄的列，这条路就该停在分析阶段。

产物：`results/derived-key.egg`、`results/derived-key-indexes.json`、`results/derived-key-analysis.json`。

## 产物

| 文件 | 内容 |
|---|---|
| `rewrite.py` | 规则改写器（rot / row），**尚未可用** |
| `results/base-indexes.json` | 真实 eggcc pass 的索引观察结果（全 0） |
| `results/base.log` | base 变体的 analyze 输出 |

复现观察器：

```sh
cargo build --offline --release --bin egg_layout
EGG_LAYOUT_GRAPHS=0 EGG_LAYOUT_INDEX_REPORT=1 \
  ./target/release/egg_layout analyze --source <program>.egg --output /tmp/idx
cat /tmp/idx/indexes.json
```

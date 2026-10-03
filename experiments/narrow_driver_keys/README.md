# 窄驱动键：能生成、能证明保守，但在可测负载上是**慢的**

回答："能否用分析结果改 rule、加 sort、生成更窄的驱动键，只在 rule level 做"。

**结论：**
1. 窄列**确实存在**（真实列基数已测）。
2. 纯 rule 级变换**可以表达**，而且能做成保守的（实测行数 = base + |key|）。
3. **但在 `fact.bril` 这个负载上它更慢**（+9%），因为 planner 本来就按表大小选驱动表，
   换驱动表不减少 join 要枚举的行，只多了一张表 + 一条 builder 规则 + 每个消费者多一个 atom。
4. 真正该测的是 raytrace 那种大图；这次把工具和坑都备好了。

## 一、先补上"width 侧"：真实列基数

之前 cell 只能给"hotness"，缺"每列去重数"。新增两个只读观察器：

- `core-relations`：`IndexCatalog::keys`、`Database::materialized_indexes`、
  `Database::column_cardinality`（每表行数 + 每列 distinct 数）
- 逐层转发到 `EGraph`，`EGG_LAYOUT_INDEX_REPORT=1` 写 `indexes.json` / `column-cardinality.json`
- 新增独立工具 `examples/column_cardinality.rs`：跑完整程序后 dump 每表每列基数

在真实的 `fact.bril` 导出程序（2,758 行 / 261 表）上测得**数据列**里最宽的：

| rows/key | rows | distinct | 列 |
|---:|---:|---:|---|
| **53.5** | 107 | 2 | `ContextOf.1` |
| 32.0 | 128 | 4 | `SubTuple.1` / `SubTuple.2` |
| 31.0 | 124 | 4 | `Add-Gets.2` |
| 23.0 | 92 | 4 | `Get.1` |
| 17.4 | 174 | 10 | `BodyContainsExpr.0` |
| 9.7 | 107 | 11 | `HasType.1` |

**方法论坑**：每张表最后两列是内部列（其中一列是 timestamp），`distinct = 1`，
第一版排名被它们污染（`BodyContainsExpr.4`、`Get.4` 之类 rows/key = 174 的"巨宽列"全是假的）。
数据列是 `0 .. n_keys + n_vals - 1`。

## 二、纯 rule 级变换（`narrow_keys.py`）

对选定的 `(Op, col)`：

```egglog
(relation __k_Op_col (<col 的 sort>))                       ; 1. 声明窄键
(rule ((= __r (Op a0 a1 ...))) ((__k_Op_col a_col))          ; 2. 投影出该列
      :ruleset <消费者所在 ruleset> :name "build_...")
(rule ((__k_Op_col v) ...原文 body...) ((...原文 head...)))  ; 3. 给消费者加键 atom
```

没有删除任何规则、没有限制任何匹配：键 atom 由它伴随的事实蕴含，
所以**按构造是保守的**。planner 按表大小选 cover（`plan_free_join`，
`free_join/plan.rs:1365-1401`），所以小键表自然成为驱动表，原表按该列被 probe。

## 三、两个调度陷阱（这是本次真正学到的）

1. **声明必须先于使用**，而这个生成程序是**声明与规则交错**的
   → 键声明要插在"首次使用它的那条规则"之前。
2. **builder 必须和消费者在同一个 ruleset 里。** 我先把 builder 放在 `always-run`
   （甚至一度追加在 `run-schedule` 之后），结果键是空的/过期的，被改写的规则直接不触发，
   **静默删掉了推导**。第一版的"漂亮结果"就是这个假象：

| 变体（错误版本） | median | rows |
|---|---:|---:|
| base | 153.5 ms | 2,758 |
| get1 | **99.2 ms** | **397** |
| — 其中 `Add-Gets` 从 124 行掉到 **0 行** | | |

行数掉 7 倍不是提速，是**算错了**。修好 builder 的 ruleset 之后：

| 变体 | median (9 次) | rows | 与 base 的关系 |
|---|---:|---:|---|
| base | 149.0 ms | 2,758 | — |
| `ContextOf:1` | 152.6 ms | 2,756 | **−2（仍不保守）** |
| `Get:1` | **161.7 ms** | **2,762** | **= base + 4**（键表正好 4 行）✅ 保守 |
| 4 个键 | 173.2 ms | 2,674 | −84（仍不保守） |

**关键教训**：在一个**分层且有界**的调度里，引入派生关系会带来**顺序义务**；
`saturate` 救不了你，因为外层 schedule 是有限次的。延迟触发的规则可能永远不再触发。
`Get:1` 之所以保守，是因为它的消费者都在能让 builder 先跑完的 ruleset 里；
`ContextOf`/`multi` 还有 ruleset 没覆盖到（默认 ruleset 的情况没处理干净）。

## 四、为什么慢

- planner 已经按**表大小**选驱动表；插一个更小的派生 atom 只是换了 cover，
  **不减少 join 要枚举的行数**（那才是成本）。
- 代价却是确定的：+1 张表、+1 条 builder 规则（每次 Op 新行都要跑）、
  每个消费者 +1 个 atom。
- 这个负载只有 2,758 行 / 261 表，没有任何空间可省。

## 五、下一步（要在大图上测）

这次的价值主要是**工具 + 排除法**：

- 工具：`examples/column_cardinality.rs`（真实列基数）、`narrow_keys.py`（rule 级变换）。
- 排除法：在 `fact.bril` 上，**投影式窄键 = 保守但更慢**。所以"加 sort 造窄键"这条路，
  在小图上不成立；它只在"驱动表远大于窄键、且 join 是主要成本"时才可能成立。

要在 raytrace 上重做，需要：
1. 导出 raytrace 的 egglog 程序（`--run-mode egglog`，约 50 s）；
2. 用 `column_cardinality` 找 rows/key 高**且**被热规则用作 probe 列的列
   （raytrace 上热列是 `Smaller`/`TCPair`/`PotentialExtractedExpr`/`Expr-size`）；
3. 用同一套 `narrow_keys.py` 生成变体，用 phase report 比 search 时间；
4. 先验证"rows = base + |key|"再比时间——这是这次学到的验收纪律。

## 产物

| 文件 | 内容 |
|---|---|
| `narrow_keys.py` | rule 级窄键变换（声明位置、builder ruleset 都已修正） |
| `results-row-counts.json` | base / 三个变体的行数对比（保守性证据） |
| `results-timings.json` | 9 次重复的 median/min |

复现：

```sh
cargo build --release --offline --example column_cardinality
./target/release/examples/column_cardinality source.egg card.json
python3 experiments/narrow_driver_keys/narrow_keys.py source.egg out.egg --spec Get:1
```

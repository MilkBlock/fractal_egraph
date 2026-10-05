# 取一条最耗时的真实规则：记录访问计划，手工改一版对照

按"选一条最耗时的真实规则，记录每个查询阶段的候选/淘汰/耗时，然后手工改一个访问计划对照"执行。
测量对象是仓库里**真实生成的 eggcc 程序** `egglog/tests/eggcc-2mm.egg`
（10,891 行、261 表、**147,959 行**终态），用本地 egglog 引擎跑它自己的 `run-schedule`。

## 一、先让"查询计划"能被打印出来（原本是 panic）

`--report-level with-plan` / `stage-info` 在这条线上直接 panic：`Plan::to_report`
对 `DecomposedPlan` 是 `todo!()`（`core-relations/src/free_join/plan.rs:254`）。
树分解（Yannakakis）路径的计划恰好是热规则会走的那种，所以原本**看不到**任何热规则的访问顺序。

已在本地 egglog 补上（只读、加法式）：把单计划的逐 stage 转换抽成
`stages_to_report(...)`，`DecomposedPlan` 把各 block 的 stage 串起来即可。
`FusedIntersectMat` 那一支原本也是 `todo!("materialization")`，改成输出可读占位而不是 panic。

```sh
./egglog/target/release/egglog eggcc-2mm.egg --report-level with-plan --save-report plan.json
```

## 二、最耗时的真实规则（288 条规则，总 search 0.56 s）

| search | 占比 | matches | stages | 规则 |
|---:|---:|---:|---:|---|
| 0.045 s | 7.9% | 15,922 | 5 | `Concat x y` + `Expr-size` 求和 → `set Expr-size` |
| 0.040 s | 7.1% | 4,614 | **10** | `Concat e1 e2` + `HasType e1 (TupleT …)` + `HasType e2 (TupleT …)` |
| 0.028 s | 5.0% | 8,959 | 6 | rewrite `Concat (SubTuple …) (SubTuple …) → SubTuple` |
| **0.023 s** | **4.1%** | **26** | **9** | **`If pred inputs then else` + 布尔检查 + 分支同型 → `HasType lhs ty`** |
| 0.020 s | 3.6% | 201 | 8 | `BodyContainsExpr` + `DoWhile` + 两个 `Get` |

第 4 条是最典型的"计划很深、产出极少"：**9 个 stage、只有 26 个 match、0.88 ms/match**
（比 µs 级规则贵约三个数量级）。

它的实际访问顺序（`s0..s8`）：

```
s0: INTERSECT If(pred) | HasType(pred)         ┐
s1: INTERSECT Base(@c1) | HasType(@c1)         ├ 三跳布尔检查
s2: INTERSECT BoolT(@c0) | Base(@c0)           ┘
s3: DRIVE (materialized) -> If(pred) -> HasType(pred)
s4: INTERSECT If(then)  | HasType(then)        ┐
s5: INTERSECT If(else)  | HasType(else)        ├ 两个分支各自取类型
s6: INTERSECT HasType(ty) | HasType(ty)        ┘ 再要求 ty 相同
s7: INTERSECT If(@c4)
s8: DRIVE (materialized)
```

**可下手处很清楚**：那个"谓词必须是 Bool"的检查被展开成 `HasType → Base → BoolT` 三跳，
而且它在这个 9 阶段计划里每次都要走一遍；它完全可以先在别处算成一个标记。

## 三、手工改一版访问计划

改法（纯 `.egg`）：把三跳布尔检查换成一次标记查找。

```egglog
(relation __boolPred (Expr))
(rule ((= p (HasType pred (Base (BoolT ))))) ((__boolPred pred))
      :ruleset type-analysis :name "build___boolPred")
(rule ((= lhs (If pred inputs then else)) (__boolPred pred)
       (HasType then ty) (HasType else ty))
      ((HasType lhs ty)) :ruleset type-analysis :name "if-typing-via-marker")
```

结果（各 5 次，中位数）：

| | 总 search | 该规则族 |
|---|---:|---:|
| base | 602.5 ms | 30.55 ms |
| **手工改版** | **566.6 ms（−6.0%）** | **9.00 ms（−70.5%）** |

**这就是本轮最重要的正面结果**：目标规则的搜索时间掉了 **70%**，总 search 掉 **6.0%**。
它证实了"计划里确实有可提前的条件，换访问计划能省下真实工作量"。

## 四、必须说清楚的三点

1. **不是精确等价的变换。** 终态行数 base 147,959 → 改版 **148,002（+43）**，
   其中标记表 38 行，**另有 +5 行**来自调度层次变化让某条规则多跑了一轮。
   所以这是"计划改动 + 轻微语义差异"，不是经过认证的等价重写。
   验收纪律应当是：先要求 `rows == base + |marker|`，这里**没有满足**，需要再收。
2. **量级是 6%，不是数量级。** 2mm 的总时间分散在 288 条规则上；单条规则再优化也只有几个百分点。
   要拿大收益，要么把这类改动铺到多条规则，要么挑一条真正主导的规则——
   raytrace 上 `terms/Smaller` 一家占 31.5%，那里同样处理的期望收益大得多。
3. **每个 stage 的"候选进入量/淘汰量"目前拿不到。** `egglog_reports::StageStats { num_candidates, num_succeeded }`
   在整条线上**从未被写入过**（`grep num_candidates core-relations/src` 无命中）；
   `to_report` 里每个 stage 的 stats 位恒为 `None`。
   要补的话，插入点在 `free_join/execute.rs` 的 `run_plan`（stage 的 probe 循环）：
   每个 stage 两个计数——枚举的 cover 行数（进入）与通过 probe 的行数（存活）。
   实现要点：计数必须挂在能活到 `to_report` 的地方（每 stage 一组原子计数），
   并且**只在 `ReportLevel::StageInfo` 时开启**，否则会给热循环加恒定开销，
   把要测的时间本身改掉。

所以本轮交付的是：**可打印的逐 stage 访问顺序 + 逐规则耗时/matches + 一次真实的手工计划对照（−70% / −6%）**；
"逐 stage 候选量"这一项需要先实现计数器，插入点和注意事项已写明。

## 产物

| 文件 | 内容 |
|---|---|
| `results/plans.json` | 前 12 条热规则的耗时、matches 与完整 stage 计划 |
| `results/comparison.json` | base vs 手工改版的 5 次重复、行数与标记表大小 |
| `make_variants.py` | 生成 base / split 两个程序 |

复现：

```sh
cd egglog && cargo build --release --offline --bin egglog && cd ..
./egglog/target/release/egglog /tmp/base.egg  --report-level with-plan --save-report base.json
./egglog/target/release/egglog /tmp/split.egg --report-level with-plan --save-report split.json
./target/release/examples/column_cardinality /tmp/base.egg  cc-base.json
./target/release/examples/column_cardinality /tmp/split.egg cc-split.json
```

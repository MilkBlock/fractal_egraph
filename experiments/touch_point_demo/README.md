# 触点（contact point）能不能省搜索？四个 .egg 的实测对照

针对这个判断做实验：

> free join 并不要求把 LHS 的 atom 数压小。查询的 relation 变多没关系，只要其中有一个关键
> relation 行少、信息多，free join 自然会选它；从它拿到绑定后，后面的 relation 就从"寻找大量
> 可能组合"变成"检查这些具体键是否存在"，于是 atom 变多而搜索空间变小。

结论：**前半句对；后半句有前提，而且"暴露触点"本身不省时间——省时间的是因为触点而能删掉的 atom。**

## 实验形状（刻意照抄 eggcc 真实的提取代价机器）

```egglog
(constructor CP (Tm i64) TC)        ;; 镜像 TCPair：输出列唯一，一个 TC 值一行
(constructor Smaller (TC TC) TC)    ;; 镜像 Smaller：两个字段是 TC **值**（指针）
```

`Smaller` 的字段是 TC 值，要拿到代价必须把它们**反查**回产生它们的 `CP` 行
（probe 输出列，不是 key 列）。这正是 raytrace 上 `Smaller` 规则的真实形状。

四个变体，数据完全相同，**做的比较完全相同**（用排序后的 `(cost1,cost2)` 多重集逐字节校验）：

| 文件 | 规则 | atom 数 |
|---|---|---|
| `baseline.egg` | `(= l (Smaller (CP t1 c1) (CP t2 c2)))` + `(<= c1 c2)` | 3（含 2 次反查） |
| `touch.egg` | 原 LHS **保留**，外加 `Costs(l,t1,c1,t2,c2)` | 4 |
| `touch_drop.egg` | 既然 Cell 已保证，**删掉**被保证的检查，只用 `Costs` | 1 |
| `inline.egg` | 表示改成 `SmallerI(Tm i64 Tm i64)`：代价内联，无需反查（需要能解构的 merge，即我上轮说的那个原语） | 1 |

## 实测（3 次中位数，egglog CLI）

| 规模 K=M | baseline | +touch | drop | inline | touch/base | drop/base | inline/base | stages (b/t/d/i) |
|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 2,000 | 1.031 ms | 2.030 ms | **0.247 ms** | **0.230 ms** | **1.97×** | **0.24×** | **0.22×** | 5/7/1/1 |
| 10,000 | 5.328 ms | 10.022 ms | **1.159 ms** | **1.112 ms** | **1.88×** | **0.22×** | **0.21×** | 5/7/1/1 |
| 30,000 | 19.192 ms | 35.077 ms | **3.461 ms** | **3.200 ms** | **1.83×** | **0.18×** | **0.17×** | 5/7/1/1 |

## 读法

1. **"外加一个触点、保留原 LHS"（你描述的第一阶段）在所有规模上都慢 1.8–2.0×**，
   阶段数从 5 涨到 7。原因是结构性的：`Costs` 与 `Smaller` 是 **1:1** 的（都是 M 行），
   它不比你已有的 atom 更窄；而 `CP` 的输出列是唯一的，反查本来就是一次点查，
   不会比正向查更贵。所以"反查 → 正向 + 点检查"净亏一个 atom。
2. **把触点用来删掉被它保证的检查（第二阶段）→ 快 4.2–5.6×**，阶段数 5→1。
3. **`drop` 与 `inline` 几乎一样快**（0.24 vs 0.22）：一旦检查被删掉，剩下的成本就是
   扫一遍那张表，触点多带的几列几乎不花钱。**收益来自"删 atom"，不来自"触点带了多少列"。**

## 所以对你的两个判断的回应

- **"atom 数量不是指标"——对。** `drop`/`inline` 只有 1 个 atom 却最快，`touch` 有 4 个 atom 最慢，
  baseline 3 个 atom 居中。数量本身没有意义。
- **"plan 会挑行少信息多的那个 relation"——有前提。** 本引擎里 `plan_gj` 的排序键是
  *每个变量所在最小 atom 的 size*（`min_sizes.sort_by_key((size, -occs))`，
  `free_join/plan.rs:1544`）。但**只有当那个小 relation 已经在 LHS 里时才成立**。
  把它"外加"进 body、同时保留原有 atoms，它不会变成驱动后变快，只是多一个要被 intersect 的 atom
  （实测 7 vs 5 stages）。
- **触点的价值是"能删掉什么"，不是"能暴露什么"。** 触点必须先**更窄**，才可能赢；
  1:1 的解析结果必然亏。要更窄，选择性必须来自 LHS 原子**之外**的信息（分析/上下文）——
  也就是"Cell 记忆化了一次昂贵推导"，而不是"重排一次便宜的 join"。

这与 raytrace 上的实测一致：`__smallerCosts` 是 1:1 物化，端到端 **+1.71 s**（亏）；
而我算的 ~6.3 s search（≈14% wall）的机会，恰好来自**删掉 `Smaller` 解析机器**
（即 `inline` 那条路），它需要"merge 表达式能解构"这一个语言特性。

## 复现

```sh
# 生成四个变体（K 个 cost pair，M 次比较）
python3 experiments/touch_point_demo/make_data.py /tmp/td 10000 10000
E=egglog/target/release/egglog
for f in baseline touch touch_drop inline; do
  $E /tmp/td/$f.egg --report-level with-plan --save-report /tmp/td/$f.rep.json
done
# 校验四个变体比较集合一致（排序后逐字节）
for f in baseline touch touch_drop inline; do
  grep -oE '\) [0-9]+ [0-9]+\) ->' /tmp/td/$f.out | sort > /tmp/td/$f.pairs
done
cmp /tmp/td/baseline.pairs /tmp/td/inline.pairs
```

`example/` 里是 K=M=200 的可读实例（每个文件 200–600 行），`results.json` 是上表的机器可读版。

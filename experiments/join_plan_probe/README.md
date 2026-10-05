# raytrace 的逐 stage 访问计划：手工改一版，看它到底有没有用

对象是**真实的 raytrace 程序**（不是 2mm）：`eggcc --run-mode=egglog --stop-after-n-passes 3
benchmarks/passing/raytrace/raytrace.rs` 导出的完整 egglog 程序（1.6 MB、710 条规则、
2 个 `run-schedule`），用本地 egglog 引擎跑它自己的调度。

## 一、raytrace 的真实热点与访问计划

导出步的 profile：**380 条规则、总 search 16.94 s、10,123 次迭代**。

| search | 占比 | matches | µs/match | 规则 |
|---:|---:|---:|---:|---|
| 1.747 s | 10.3% | 42,713 | 40.9 | `PotentialExtractedExpr` + `ExtractedExpr` + `(< cost oldcost)` → `set` |
| 1.252 s | 7.4% | 68,852 | 18.2 | `Smaller (TCPair t1 c1) (TCPair t2 c2)` + `(<= c1 c2)` → `union t1` |
| 1.236 s | 7.3% | 75,568 | 16.4 | `Smaller` + `(> c1 c2)` → `union t2` |
| 0.857 s | 5.1% | 16,734 | 51.2 | `Bop o e1 e2` + 两个 `ExtractedExpr` |
| 0.726 s | 4.3% | 6,160 | 117.8 | `Concat e1 e2` + 两个 `ExtractedExpr` |
| 0.722 s | 4.3% | 329,668 | 2.2 | `Concat x y` + `Expr-size` 求和 |
| 0.537 s | 3.2% | 956,221 | 0.6 | `Get tup i` + `Expr-size` |

`Smaller` 两条的访问计划（实测，5 stages）：

```
s0: INTERSECT TCPair(@c2) | Smaller(@c0)   <- 把 Smaller 第 1 个字段解析回 TCPair
s1: INTERSECT TCPair(@c2) | Smaller(@c1)   <- 第 2 个字段同样
s2: DRIVE TCPair[t1@c0,cost1@c1]
s3: INTERSECT TCPair(cost2@c1)
s4: INTERSECT Smaller(@c2)
```

**结构性问题看得很清楚**：`Smaller` 的字段类型是 `TermAndCost`（e-class 值），
要拿到 cost 必须反查"哪个 `TCPair` 产生了这个值" —— 那是 `TCPair` 的**输出列 `@c2`**，
而输出列**不是 key 列**。于是**同一个 5 阶段的解析 join 被两条比较规则各做了一遍**
（18.2 和 16.4 µs/match）。

（另外两条 `ExtractedExpr` 规则是同样的形状：`PotentialExtractedExpr(@c1)` 反查 `TCPair(@c2)`。）

## 二、手工改一版：把解析结果物化一次

```egglog
(relation __smallerCosts (TermAndCost TermAndCost i64 TermAndCost i64))
(rule ((= l (Smaller (TCPair t1 c1) (TCPair t2 c2))))
      ((__smallerCosts l (TCPair t1 c1) c1 (TCPair t2 c2) c2))
      :ruleset terms-helpers-helpers)         ;; 与消费者同 ruleset，保证先调度
(rule ((__smallerCosts l a c1 _b c2) (<= c1 c2)) ((union l a)) :ruleset terms-helpers-helpers)
(rule ((__smallerCosts l _a c1 b c2) (> c1 c2))  ((union l b)) :ruleset terms-helpers-helpers)
```

### 在导出步上（standalone，各 3 次）：

| | 总 search | `Smaller` 家族 | 该家族 µs/match |
|---|---:|---:|---:|
| base | 16.67 s | 2.46 s | 18.2 / 16.4 |
| 改版 | **14.99 s（−10.1%）** | **1.25 s（−49%）** | **8.7** |

### 但在真实 3-pass 流水线里（`eggcc --run-mode=optimize`）：

| 变体 | egglog | search | rebuild | 迭代数 | pass2 行数 | **进程 wall** |
|---|---:|---:|---:|---:|---:|---:|
| pristine | 39.08 | 31.73 | 5.66 | 23,378 | 697,997 | 49.58 |
| fold only（只折 `<=` + 删死规则） | **35.12** | 28.18 | 5.49 | 23,380 | 697,944 | **45.27** |
| **plan change** | 35.71 | **27.96** | 6.11 | **25,123** | **770,156** | 46.98 |

**结论：改版把 search 压到最低，但在完整流水线上并没有赢过更简单的那一版。**
多出来的代价是实打实的：`__smallerCosts` 让终态多 **72,159 行**（标记表 73,964 行），
迭代数从 23,380 涨到 **25,123（+7.5%）**，rebuild 从 5.49 涨到 **6.11 s**。
省下的 search（0.21 s）远小于多付的 rebuild + 迭代开销，wall 反而比 fold-only 差 1.7 s。

三个变体抽出的程序**逐字节相同**（sha256 `d18709571e6c…`），所以这次计划改动在 raytrace 上
**没有改变语义**；差的是纯开销结构。

## 三、这次测出来的方法论教训

1. **standalone 导出步会高估收益。** 单步看 −10.1%，进流水线后连更简单的 fold 都不如。
   原因：导出步是从头跑的程序，而流水线的第 3 pass 是在前两 pass 建好的图上跑的；
   而且单步测不到"标记表让迭代数变多"这件事。
2. **验收必须看 rebuild + 迭代数 + 终态行数，不能只看 search。** 这次的假收益就藏在
   "search 降了"和"rebuild/迭代涨了"的差额里。
3. **物化派生表在 raytrace 上不划算**，因为标记表本身与 `Smaller` 同数量级（73,964 ≈ 72,255 个
   `Smaller` 节点），1:1 的物化不可能省，只会多一份维护成本。要真省，必须让派生表**比源表小**
   （真正的"更窄"），比如只标记"代价会变好"的那部分节点。
4. **逐 stage 的候选/淘汰计数仍然拿不到**：`StageStats` 在整条线上从未被写入
   （`grep num_candidates core-relations/src` 无命中）。有了它才能提前判断
   "哪一步在大量淘汰"，从而避免这类"改了才发现没收益"的试验。

## 产物

| 文件 | 内容 |
|---|---|
| `results/raytrace-plans.json` | 前 6 条热规则的耗时、matches 与完整 stage 计划 |
| `results/raytrace-comparison.json` | 三变体的 egglog/search/rebuild/迭代/行数/wall |
| `make_plan_variant.py` | 生成 raytrace 的 plan-change 变体 |

复现：

```sh
cd /Users/mineralsteins/Repos/egg_related/check_statewalk
./target/release/eggcc --run-mode=egglog --stop-after-n-passes 3 \
  benchmarks/passing/raytrace/raytrace.rs > /tmp/rt.egg
cd /Users/mineralsteins/Repos/egg_related/egg_layout
EGGLOG_PLAN_FILTER=TCPair ./egglog/target/release/egglog /tmp/rt.egg \
  --report-level with-plan --save-report /tmp/rt-plan.json
python3 experiments/join_plan_probe/make_plan_variant.py /tmp/rt.egg /tmp/rt-split.egg
./egglog/target/release/egglog /tmp/rt.egg       --report-level time-only --save-report a.json
./egglog/target/release/egglog /tmp/rt-split.egg --report-level time-only --save-report b.json
```

---

## 附：怎么读这些 stage 计划

### 记号

```
s0: INTERSECT TCPair(@c2) | Smaller(@c0)
s2: DRIVE TCPair[t1@c0,cost1@c1]  ->  Other(x@c3)
```

- `c0/c1/...` 是**该表自己的列号**（0 起）。constructor 的**输入列在前，输出列在后**，
  再后面是引擎内部列（其中一列是 timestamp，规则的列引用里不出现）。
- `@xxx` 是规划器的**内部变量**（用户没给它起名）：`@TCPair11` = "第 11 号 `TCPair` atom 的输出值"，
  `@Smaller2` = "第 2 号 `Smaller` atom 的输出值"。
- `INTERSECT a(colA) | b(colB) | …` = **generic join 的一步**：在这些 atom 的这些列上按**同一个变量**求交。
  它会把每个 atom 的子集缩小到"在该变量上有对手"的行。
- `DRIVE cover[...] -> probe(...)` = **FusedIntersect**：以 `cover` 为驱动，枚举它的行、绑定它的列，
  再按列去探测 `->` 后面的 atom（这里 `to_intersect` 为空，表示只需枚举 cover 自己）。

### 相关表的实际形状（raytrace 导出步）

| 表 | 声明 | 行数 | 各列 distinct |
|---|---|---:|---|
| `Smaller` | `(constructor Smaller (TermAndCost TermAndCost) TermAndCost)` | 73,961 | c0:35,267 / c1:37,885 / **c2:37,625（输出）** |
| `TCPair` | `(constructor TCPair (Term i64) TermAndCost)` | 63,071 | c0:63,071 / c1:15,712 / **c2:63,071（输出）** |
| `ExtractedExpr` | `(function ExtractedExpr (Expr) TermAndCost :merge (Smaller old new))` | 7,422 | c0:7,422（key）/ c1:2,993（值） |

关键：`TermAndCost` 是 **e-class 值**。`Smaller` 的两个字段存的是"指向某个 `TCPair` 结果的值"，
而 `TCPair` 的**输出列 c2 是唯一的**（63,071 个不同值 / 63,071 行）——所以"反查是哪个 `TCPair`
产生了这个字段值"本质上是**一次字典查找**，只是方向在**值列**（不是 key 列 `(Term,i64)`）上。

### 逐 stage 走一遍（玩具例子）

设图里有：

```
TCPair(TermConst(5), 1)  -> TC#100
TCPair(TermBop(Add,…), 3) -> TC#200
Smaller(TC#100, TC#200)  -> TC#300          ; 某次 ExtractedExpr 的 merge 产生的
```

规则要做的：`Smaller` 的第 1 个字段（`TC#100`）和第 2 个字段（`TC#200`）**分别反查回它们的 TCPair**，
比较两者的 cost（1 和 3），赢家与 `lhs`（`TC#300`）合并。

```
s0: INTERSECT TCPair(@c2) | Smaller(@c0)
    在值 TC#100 上把「TCPair 输出列」与「Smaller 第 1 字段」求交
    -> 锁定第 1 行 TCPair 与那行 Smaller
s1: INTERSECT TCPair(@c2) | Smaller(@c1)
    在值 TC#200 上做同样的事 -> 锁定第 2 行 TCPair
s2: DRIVE TCPair[t1@c0, cost1@c1]
    枚举第 1 行 TCPair，绑定 t1 = TermConst(5), cost1 = 1
s3: INTERSECT TCPair(cost2@c1)
    从第 2 行 TCPair 绑定 cost2 = 3
s4: INTERSECT Smaller(@c2)
    绑定 lhs = TC#300
    （规则体的 (<= cost1 cost2) 在绑定完成后求值，1<=3 成立）
    （head: union(TC#300, TCPair(TermConst(5),1))）
```

### 为什么这个计划贵

1. **反查走的是值列**：`Smaller.c0/c1` 是 `TermAndCost` 值，要拿 cost 必须沿 `TCPair.c2` 反查。
   引擎的主键索引在 `(Term,i64)` 上，按值列查需要次级索引，而次级索引是否被缓存取决于子集形状
   （见 `experiments/rule_index_shaping`：只有子集又大又密才走缓存索引）。
2. **同一个反查做了两遍**：s0 和 s1 形状完全相同，只是一个接 `c0` 一个接 `c1`。
3. **两条规则各做一遍**：`<=` 和 `>` 是两条独立规则，各自完整跑这套 5 阶段 join
   （68,852 + 75,568 次匹配，18.2 / 16.4 µs/match）。
4. **根因在表示**：`Smaller` 存的是**指针**（e-class 值）而不是 cost 本身。
   如果 `Smaller` 能内联 `(Term i64 Term i64)`，这个 5 阶段 join 会塌缩成"扫一张表 + 比较两个内联整数"——
   不需要反查、不需要标记表。做不到的原因是 `:merge (Smaller old new)` 只能拿到两个 `TermAndCost` 值，
   **merge 表达式里无法解构**它们（规则体可以，merge 表达式不行），除非有一个能解构的原语。

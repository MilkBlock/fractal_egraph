# vanilla egglog 中 300 元素加法的交换律节点数

日期 2026-09-28。测的是 **pristine egglog 本体**，不是 egg_layout：引擎为不可变标签
`egglog-baseline`（`c7553a6`，上游 `ebba7bb`），通过独立 public-API consumer 运行，
timed path 里没有 tracing、RipSat 分析、graph export 或 egg_layout 代码。
harness 源码见 `experiments/native_compare/main.rs`（枚举 e-node / e-class）与
`probe.rs`（每次迭代后 `(print-size)` 读表行数）。

每个配置的初始程序只有 300 个不同叶子 `(Num 0) … (Num 299)` 和 299 个 `Add` 位置：

* `chain`：右嵌套 `(Add (Num 0) (Add (Num 1) … (Num 299)))`
* `balanced`：同样 300 个叶子的完全二叉树

## 结果

### 只有交换律 `(rewrite (Add a b) (Add b a))`，跑到饱和

| 形状 | Add e-node | Num e-node | Math 输出节点合计 | e-class | 匹配数 | 引擎内用时 |
|---|---:|---:|---:|---:|---:|---:|
| chain | **598** | 300 | **898** | **599** | 598 | 0.044 s |
| balanced | **598** | 300 | **898** | **599** | 598 | 0.008 s |

交换律单独作用不爆炸：每个原有 `Add` 被反向一次后即饱和，Add 节点恰好 `2(n-1)`，
e-class 为 `2n-1`。形状不影响结果。

### 结合律 + 交换律（AC），逐步 `(run k)`

| 迭代 | chain Add e-node | chain 峰值 RSS | balanced Add e-node | balanced 峰值 RSS |
|---:|---:|---:|---:|---:|
| 1 | 1,194 | 8 MiB | 896 | 8 MiB |
| 2 | 4,760 | 10 MiB | 3,120 | 8 MiB |
| 3 | 31,618 | 147 MiB | 18,180 | 12 MiB |
| 4 | 438,077 | 276 MiB | 204,468 | 196 MiB |
| 5 | **16,738,660** | **4.75 GiB (9.6 s)** | **5,517,289** | **1.65 GiB (1.8 s)** |

第 5 轮之后按 `node_ceiling=3,000,000` 有意停止，没有继续跑第 6 轮；chain 每轮约 ×38，
下一轮量级在 6 亿节点、几十 GiB 以上。**AC 在 300 元素上不可能跑到饱和。**

饱和目标（`saturation_check.py`，n=2…8 实测）：

饱和目标（`saturation_check.py`，n=2…10 实测，chain 形状）：

| n | e-class 实测 | 预测 `2^n − 1` | Add e-node 实测 | 预测 `3^n − 2^(n+1) + 1` | 饱和用时 |
|---:|---:|---:|---:|---:|---:|
| 2 | 3 | 3 | 2 | 2 | 0.001 s |
| 3 | 7 | 7 | 12 | 12 | 0.001 s |
| 4 | 15 | 15 | 50 | 50 | 0.001 s |
| 5 | 31 | 31 | 180 | 180 | 0.001 s |
| 6 | 63 | 63 | 602 | 602 | 0.003 s |
| 7 | 127 | 127 | 1,932 | 1,932 | 0.006 s |
| 8 | 255 | 255 | 6,050 | 6,050 | 0.020 s |
| 9 | 511 | 511 | 18,660 | 18,660 | 0.043 s |
| 10 | 1,023 | 1,023 | **57,002** | 57,002 | 0.114 s |

饱和时每个非空叶子子集都是一个 e-class，所以 n=300 时仅 e-class 就有
`2^300 − 1 ≈ 2.0 × 10^90` 个；Add e-node 有 `3^300 − 2^301 + 1 ≈ 1.4 × 10^143` 个。
这两个数只能写表达式，不能枚举；n=10 的 57,002 个则可以完整跑完。

### n=10 的逐步增长（AC，chain）

| 迭代 | Add e-node | e-class | 说明 |
|---:|---:|---:|---|
| 1 | 34 | 27 | |
| 2 | 120 | 62 | |
| 3 | 588 | 245 | |
| 4 | 3,345 | 1,154 | |
| 5 | 15,492 | 3,763 | |
| 6 | 40,732 | 5,984 | e-class 过冲峰值 |
| 7 | 59,446 | 4,199 | Add 节点过冲峰值 |
| 8 | 57,026 | 1,028 | |
| 9 | **57,002** | **1,023** | 饱和（`3^10−2^11+1` / `2^10−1`） |

饱和过程不是单调的：union 会把已建节点并掉，所以 e-class 在第 6 轮冲到 5,984 后回落
到 1,023，Add 节点在第 7 轮冲到 59,446 后回落到 57,002。总匹配约 1.25 M 次，
最后一轮用时约 0.12–0.15 s，峰值 RSS 约 241 MiB（含 graph export）。
`balanced` 形状同样在第 9 轮落到 57,002 / 1,023，只是前几轮更慢。

## 三元素：把「所有组合」写成一条 rule

种子 `Add (Num 0) (Add (Num 1) (Num 2))`。四种写法，都在 vanilla egglog 上跑到饱和，
`matches` 都是 6：

| 编码 | Add e-node | e-class | 是否等于 AC 闭包 |
|---|---:|---:|---|
| 通用两条 rewrite（assoc + comm） | 12 | 7 | 基准 |
| 一条 rule，head 罗列 12 种写法，叶子用 `(Num x)` 限定 | 18 | 10 | 否 |
| 一条 rule，模式变量自由 `a/b/c` | 18 | 10 | 否 |
| 一条 rule，叶子限定 + 同时 union 三个二元子和（`three_elements.egg`） | **12** | **7** | **是** |

AC 闭包的结构是：3 个二元类各 2 个节点（`x+y` 与 `y+x`）+ 1 个三元类 6 个节点
（3 种切分 × 2 种顺序），加 3 个 `Num` → 12 个 Add e-node / 7 个 e-class。最后一行与
通用规则跑出来的 constructor 节点多重集**逐节点一致**（用 `term` 归一化后比较，
不是只比数量）。

两个反例的原因不同：

* **只在 head 罗列 12 种写法**：`(Add (Num x) (Num y))` 这类二元子和被建出来了，
  但没有被 union 成同一个类。于是 6 个二元节点各占一个类，三元节点有 12 个
  （每种切分两种顺序用到了不同的二元类）→ 18 / 10。「写出所有项」不等于「断言它们相等」。
* **模式变量不限定叶子**：第一轮是对的，但 root 所在 e-class 已经把所有项合并；
  之后 body 会用这个整体类去绑定 `c`（只要求第二操作数是 `Add`），head 造出
  `0+(1+(0+1+2))`、`(0+(0+1+2))+2` 这类原本不属于三元素和的新项并继续扩散
  → 同样 18 / 10，但多余节点是全新的和，不是漏 union。seed 换成 `0+(1+2)` 时不
  容易看出来，因为两种机制的数量恰好撞在一起。

文件：`three_elements.egg`（正确版）、`three_elements_unguarded.egg`（自由变量反例）。
直接跑：

```sh
out/vanilla-commutativity/bin/probe experiments/vanilla_commutativity/three_elements.egg
# ((Add 12) (Num 3))
```


## 复现

需要一个干净的 `egglog-baseline` checkout（只读，不修改）：

```sh
BASELINE=/Users/mineralsteins/.codex/worktrees/native-baseline/egg_layout
python3 experiments/vanilla_commutativity/build.py --baseline "$BASELINE"
python3 experiments/vanilla_commutativity/bench.py --n 300 --ac-steps 6 --node-ceiling 3000000
python3 experiments/vanilla_commutativity/bench.py --n 10 --ac-steps 9 --out out/vanilla-commutativity/n10
python3 experiments/vanilla_commutativity/saturation_check.py --max-n 10
```

原始数字：`results.json`（n=300）、`results_n10.json`（n=10 逐步）、`saturation.json`
（n=2…10 饱和）；本目录里的副本即上表来源，运行时脚本也会写到被忽略的
`out/vanilla-commutativity/`。每步用时是单次进程测量，不是多次基准的平均值；
峰值 RSS 来自 `/usr/bin/time -l`，区分单进程；结果与机器相关。`--n` 是参数。

## 边界

* 只覆盖「叶子数 = n、Add 位置 = n−1」这一族初始项。
* 只测 vanilla egglog 的节点/类别数与耗时，不涉及 egg_layout 的 ripen/comb 结果，
  也不主张任何加速。
* AC 的迭代边界是人给的：`(run 5)` 完成后按节点上限停止，不是引擎自己饱和。

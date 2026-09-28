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

| n | e-class 实测 | 预测 `2^n − 1` | 饱和 Add e-node |
|---:|---:|---:|---:|
| 2 | 3 | 3 | 2 |
| 3 | 7 | 7 | 12 |
| 4 | 15 | 15 | 50 |
| 5 | 31 | 31 | 180 |
| 6 | 63 | 63 | 602 |
| 7 | 127 | 127 | 1,932 |
| 8 | 255 | 255 | 6,050 |

饱和时每个非空叶子子集都是一个 e-class，所以 n=300 时仅 e-class 就有
`2^300 − 1 ≈ 2.0 × 10^90` 个；Add e-node 更多。这个数只能写出表达式，不能枚举。

## 复现

需要一个干净的 `egglog-baseline` checkout（只读，不修改）：

```sh
BASELINE=/Users/mineralsteins/.codex/worktrees/native-baseline/egg_layout
python3 experiments/vanilla_commutativity/build.py --baseline "$BASELINE"
python3 experiments/vanilla_commutativity/bench.py --n 300 --ac-steps 6 --node-ceiling 3000000
python3 experiments/vanilla_commutativity/saturation_check.py
```

原始数字：`results.json`、`saturation.json`（本目录，同上表来源；运行时脚本也会写到
被忽略的 `out/vanilla-commutativity/`）。每步用时是单次进程测量，不是多次基准的平均值；
峰值 RSS 来自 `/usr/bin/time -l`，区分单进程；结果与机器相关。`n` 是参数，换 `--n` 即换元素个数。

## 边界

* 只覆盖「叶子数 = n、Add 位置 = n−1」这一族初始项。
* 只测 vanilla egglog 的节点/类别数与耗时，不涉及 egg_layout 的 ripen/comb 结果，
  也不主张任何加速。
* AC 的迭代边界是人给的：`(run 5)` 完成后按节点上限停止，不是引擎自己饱和。

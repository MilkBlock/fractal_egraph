# 因子化 Spine：不展开候选的边界矩阵

这是一个 **独立、精确的 CPU 原型**。使用真实 math microbenchmark 的四条
rewrite 和本仓库 native egglog 验证；没有修改引擎，没有接入生产 Bake，也不报告 GPU 加速。
候选语法是为四规则子集手工推导的，不是通用自动因子化算法。

## 输入与 Cell

`rules.egg` 的 rewrite 逐字取自 `egglog/tests/math-microbenchmark.egg:19,20,33,34`：
Add 交换、Mul 交换、分配律和提取公因式。`source.egg` 是可单独执行的输入。

```text
C0 = x*(a+b)
C1 = x*(c+d)
C2 = y*(a+b)
C3 = z*(e+f)
```

每个 Cell 单独饱和后有 13 个 Math e-node / 7 个 e-class，根的完整有限项集合有 12 个元素。
本实验将这些根表达式作为候选 Spine；每个 Spine 单独 saturation 后都生成同一个完整 Cell。
不是任意带环 egraph 的所有 Spine 枚举方法。

## 1. 不枚举 12 行，而保存一个 union/product DAG

以 C0 为例：

```text
S = Add(a,b) | Add(b,a)
P = Mul(x,a) | Mul(a,x)
Q = Mul(x,b) | Mul(b,x)

F = Mul(x,S) | Mul(S,x)       # 2×2 = 4 个因式形式
E = Add(P,Q) | Add(Q,P)       # 2×2×2 = 8 个展开形式
C0.spines = F | E            # 共 12 个候选
```

`|` 是替代选择。构造器对两个孩子的候选做笛卡尔积。相同子表达式只存一份。
三个变量名必须不同，保证这份特定语法的选择分支没有重复语法项；重复变量需要先去重，
本原型明确拒绝，不能继续按乘积计数。
跨 Cell 也共享：C0 与 C2 共用 `S`。相关的 binding 始终留在同一构造器／元组中。

实际测量（同一紧凑 JSON 编码；含根引用，不含程序和 native 验证文件）：

| 表示 | 节点数 | 字节数 |
| --- | ---: | ---: |
| 48 个完整树逐个保存 | 304 | 3,145 |
| 普通 term hashcons，仍列出 48 个候选根 | 79 | 1,217 |
| 因子化 DAG，只列出 4 个家族根 | 70（含 Choice） | 938 |

因子化比普通 hashcons 少约 22.9% 的这部分序列化字节；不是 RSS，也不是整体库压缩率。
这个超小实例不能证明大规模时仍保持该比例。

## 2. 声明要计算的真实边界交互

外界提供一个新的 coarse fact：`Add(root(Ci), root(Cj))`。
检查原规则：

```egg
(rewrite (Add (Mul k u) (Mul k v)) (Mul k (Add u v)))
```

**没有外层 Add，仅仅共享公因子并不构成已发生的 match。** 此例限定为根上的 Mul
触点，不声称覆盖所有非 root、多方、guard 或动态 union 交互。

直接在 DAG 上计算 `Mul(k,u)` 的投影：

- OR：合并孩子的 binding 计数。
- 根 Mul：取两个孩子的 canonical e-class，计数为孩子候选数之积。
- 根不是 Mul：该方案没有贡献。

整个 compact 路径没有调用候选枚举。闭包和 canonical e-class 注释是在 native Bake
验证阶段得到的；这里没有省略这项准备成本，也没有声称所有工作只靠一次矩阵乘法。

C0 的投影只有两条：

```text
(k=x,   u=a+b) : 2
(k=a+b, u=x)   : 2
```

虽然展开分支 E 的根不是 Mul，F 已提供了本边界规则所需的等价表示。
计数中的 2 来自 `a+b` 与 `b+a` 两种完整 Spine；它们对应同一个 canonical binding。

## 3. 直接计算矩阵

Q 的列是完整的 `(k,u)` 元组。F 把元组列投影到公因子 k：

```text
K = F Fᵀ                # 元组之间的相同公因子关系
V = Q F
S = Q K Qᵀ = V Vᵀ
```

按 `[a+b, c+d, e+f, x, y, z]` 排列列坐标：

```text
      a+b c+d e+f  x  y  z
C0     2   0   0  2  0  0
C1     0   2   0  2  0  0
C2     2   0   0  0  2  0
C3     0   0   2  0  0  2
```

计算得到：

```text
S = [ 8 4 4 0
      4 8 0 0
      4 0 8 0
      0 0 0 8 ]
```

- C0、C1 通过 x 相接：`x*((a+b)+(c+d))`。
- C0、C2 通过 **复合结构 a+b** 相接：`(a+b)*(x+y)`。
- C1、C2 的形状很像，但公因子没有相等，结果为 0。
- C3 只在本次查询里与自己兼容。

`S01=4` 是 2×2 对 Spine，不是 4 个不同 logical match，更不是 4 次已提交 mutation。
通过 Q 的联合元组恢复 `(k,u,v)` 后，对应 1 个不同 binding。全部 16 个有序 Cell 对
共 8 对兼容、12 个不同 binding；对角线每项含两种公因子。

仅保存 V 会丢失 RHS 所需的 u。本程序保留压缩后的 Q，因此不会把两个不同候选的
k 和 u 错拼。恢复的是 8 条投影记录之间的连接，不是 48 个完整 Spine 的笛卡尔积。

## 4. 验证范围

1. 从 native 饱和 egraph 独立枚举所有根项，确认每个候选家族恰好完整覆盖 12 项。
2. 48 个 Spine 分别作为唯一输入 saturation；逐 constructor 行及等价分区比较，均为 13/7。
3. 单独穷举 48×48=2,304 个候选对，矩阵每项一致。
4. native egglog 在已饱和父 Cell 上注入外层 Add，运行真实 LHS 查询；12 个完整 binding 完全一致。
5. 原始 rewrite 实际执行一轮，12 条目标等价检查通过。
6. 测试将 `Family.enumerate` 替换成直接抛错函数，compact 计算仍然通过。
7. 额外测试：相关 binding 不产生伪笛卡尔积；不属于同一 e-class 的 Choice 被拒绝。

计数是注入外层 Add 后、重新 saturation 前的边界查询。不能把零项解释为任意后续推理后
也永远无法连接。这个实验没有修改规则调度或实现 GPU kernel。

## 运行

```sh
python3 experiments/cell_spine/factorized_boundary/demo.py \
  --output out/factorized-spine-next

# 已构建 native runner 时：
python3 experiments/cell_spine/factorized_boundary/demo.py \
  --output out/factorized-spine-next --skip-build

python3 -m unittest experiments.cell_spine.factorized_boundary.test_demo -v
```

输出目录必须不存在。只依赖 Python 标准库和已有 Rust workspace；默认离线编译 runner。
`index.html` 展示因子化语法、Q/V/S 矩阵，并可选择左右 Cell 查看实际 binding 与 graft 结果。
`compact.json` 在任何候选枚举之前写出。`candidates.json`、`equivalence/` 仅用于验证。
`graft-check.egg` 可以交给 `target/release/examples/native_math_baseline` 单独执行。
`results.json` 保存一次结果快照；原始报告另记录 runner 二进制 SHA256 和规则 SHA256。

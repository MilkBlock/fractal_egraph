# RipenInput 模板因子化：修正模板／实例混淆

**修正旧结论：** 上一版 `x(a+b)、x(c+d)、y(a+b)、z(e+f)` 是一个模板的四个实例。
将它们当成四份带字母的结构，测得的 `1,217 → 938 B` 不能说明相对于已有
RipenInput 参数化的增益。旧代码与数字留在 Git 历史中，不再作为当前实验结论。

本版是独立 CPU 原型，用 native egglog 验证；不是 GPU 实现或生产 Bake 的加速改动。
使用 `egglog/tests/math-microbenchmark.egg:19,20,33,34` 原样的四条 rewrite：
Add/Mul 交换、分配律、提取公因式。没有修改执行引擎。

## 两个确实不同的模板，四份实例 binding

模板中没有具体字母。以 `(RipenInput i64)` 作为不受这四条规则改写的输入叶子：

```text
T0 = p0 * (p1 + p2)
T1 = p0 * (p1 + p2) + p0 * p3
p_i = RipenInput(i)
```

| 模板 | 输入数 | 单独饱和 e-node / e-class | 完整根 Spine 数 |
| --- | ---: | ---: | ---: |
| T0 | 3 | 13 / 7 | 12 |
| T1 | 4 | 22 / 11 | 56 |

| 实例 | 模板 | binding |
| --- | --- | --- |
| I0 | T0 | x,a,b |
| I1 | T1 | x,a,b,c |
| I2 | T0 | z,a,b |
| I3 | T1 | y,d,e,f |

模板去重采用首次出现的变量重命名。它不是通用槽位对称性求解器。
为避免同一语法被不同 choice 重复计数，本次实例域限定为每份 binding 内变量互不相同；
不同实例之间允许共享变量。aliased input 明确拒绝，不会静默套用原候选数。

## 从原生闭包读取因子化表示

不再手写 12/56 个候选。`Family.from_graph` 读取真实参数化闭包：

- 一个 e-class 的 enode 选择对应 OR；
- 一个构造器的孩子组合对应乘积；
- 参数只保存 Input 编号；
- 多个模板共用一个结构字典，相同完整子电路只存一次。

这实际上利用了 egraph 已有的 e-class 共享。此有限 DAG 示例可精确表示所有根项；
带环闭包需递归表示，当前原型拒绝，不把无限集合枚举出来。

为了阅读，可以把两份电路写成：

```text
S = Add(p1,p2) | Add(p2,p1)
P = Mul(p0,p1) | Mul(p1,p0)
Q = Mul(p0,p2) | Mul(p2,p0)
T0 = Mul(p0,S) | Mul(S,p0) | Add(P,Q) | Add(Q,P)
   = 2 + 2 + 4 + 4 = 12

Z = Mul(p0,p3) | Mul(p3,p0)
H = Add(S,p3) | Add(p3,S)                  # 4
T1 = Add(T0,Z) | Add(Z,T0) | Mul(p0,H) | Mul(H,p0)
   = 12×2 + 2×12 + 4 + 4 = 56
```

T0 的整个 17 节点子电路直接被 T1 引用。这里是两个不同模板共享，不是重复实例共享。

## 公平的存储账

相同紧凑 JSON 序列化，统计完整模板库，包括根引用：

| 表示 | 结构数 | 模板字节 |
| --- | ---: | ---: |
| 已擦除变量的全部 Spine，普通 term hashcons | 84 节点 | 1,340 |
| 两个模板分别因子化 | 46 节点（含 OR） | 638 |
| 两个模板联合因子化 | 29 节点（含 OR） | 403 |
| 直接保存共享 RipSat egraph | 22 enode / 11 e-class | 382 |

四份实例 binding 独立占 **165 B**，各方案必须加同样的这部分，不能把它算成结构增益。
编译后的两份符号边界投影另外占 **283 B**。执行时的实例 Q/V 矩阵和查询工作区没有
混入模板序列化数字；这些数字不是总 RSS 或整体运行时存储收益。

**403 B 没有优于已有 RipSat egraph 的 382 B。** 联合因子化和 egraph 在这个实验里
具有相同的逻辑共享；序列化大小差异是布局，不是新发现的结构压缩。
相对于“枚举参数化候选再做 term hashcons”确实更小，但相对于已经保存的共享闭包，
这里验证的价值是直接编译边界计算，而不是新的存储突破。

## 不展开候选的边界计算

每个模板只编译一次根上的 `Mul(k,u)` 投影，计数乘法来自孩子候选数：

```text
T0: (p0, p1+p2):2，(p1+p2, p0):2
T1: (p0, (p1+p2)+p3):4，((p1+p2)+p3, p0):4
```

实例只代入这两份符号投影，不重新生成 candidate DAG，也不查询全部 concrete native 行。
本例投影键的域只有变量和二叉 Add，按 Add 交换律规范化，不包含结合律；这不是任意
Math 表达式／动态 union 的 canonicalizer。原生对照逐一核验了这些键。

仍必须由外部提供 `Add(root(Ii), root(Ij))`，才能触发原始规则：

```egg
(rewrite (Add (Mul k u) (Mul k v)) (Mul k (Add u v)))
```

Q 的列保留完整 `(k,u)`，F 投影到公因子 k；不能从独立 k 集合和 u 集合拼 binding。

```text
V = QF
K = FFᵀ
S = QKQᵀ = VVᵀ

S = [ 8  8  4  0
      8 32  0  0
      4  0  8  0
      0  0  0 32 ]
```

- I0(T0) 与 I1(T1) 通过 x 交互：2×4=8 对 Spine，只有一个不同 binding。
- I0 与 I2 通过复合因子 a+b 交互：2×2=4 对 Spine。
- I1 与 I2 无兼容公因子，结果为 0。

矩阵元素是候选对数量，不是不同 match，更不是 committed mutation。
查询在注入外层 Add 后、再次 saturation 前进行。零项不意味着以后永远无法交互。
只验证根 Mul 触点，不声称覆盖任意非 root 或多方交互。

## 正确性与反回归

- 从 native 参数闭包独立枚举根项，确认两份电路完整覆盖 12+56 个候选。
- 68 个参数 Spine 分别 saturation，完整 constructor 行及等价分区与模板一致。
- 验证路径代入四份 binding，穷举 136²=18,496 个实例候选对，与矩阵逐项一致。
- 原生 LHS 查询得到 12 个不同完整 binding，全部与紧凑计算一致。
- 原始 rewrite 实际执行一轮后，12 条目标等价检查通过。
- 禁止 `Family.enumerate` 和 `Graph.enumerate_terms`，紧凑计算仍然通过。
- 四个重命名旧例子只能得到一个模板；实例复制 100 次也不能增加模板数量。
- 验证联合 binding 相关性、已有 RipSat 存储对照、空投影和不支持的 alias 输入。

## 复现

```sh
python3 experiments/cell_spine/factorized_boundary/demo.py \
  --output out/parameterized-spine-next

# 已构建 native runner 时可加 --skip-build。
python3 -m unittest experiments.cell_spine.factorized_boundary.test_demo -v
```

输出目录必须不存在。默认离线构建 Rust runner；Python 只用标准库。
`index.html` 展示两种模板、四份 binding、公平存储表、矩阵以及可选择的交互结果。
`compact.json` 在任何候选枚举之前生成；`bake/t*/` 是两份真正的 RipenInput 原生闭包。
`validation/` 和 `candidates.json` 只用于校验。`results.json` 是一次结果快照，
不是性能基准。旧页面路径也已换成修正版内容，避免继续展示错误计量。

# 共同匹配 Spine：跨 Cell、root 与非 root 的计算复用

本实验沿用 `factorized_boundary/rules.egg` 的四条真实 math microbenchmark rewrite，
以及两个真正不同的 RipenInput 模板：

```text
T0 = p0*(p1+p2)
T1 = p0*(p1+p2)+p0*p3
```

目标是复用匹配计算，不是测量存储压缩。独立 Python CPU 原型，原生 egglog 做完整
LHS binding 对照；未修改执行引擎，未实现 GPU kernel，未证明真实运行时间加速。

## 自动提取什么

`examples/lhs_ast` 用 egglog 自身的 parser 导出原始 rewrite LHS。没有另写 .egg parser。
编译器提取 LHS 的 constructor 子模式和根 constructor 骨架，进行变量重命名归一化，
但保留变量重复、参数顺序和类型。当前自动得到：

```text
S0 = Add(v0,v1)
S1 = Mul(v0,v1)
S2 = Mul(v0, Add(v1,v2))
S3 = Add(Mul(v0,v1), Mul(v0,v2))
```

S2 引用 S1 的根结构、S0 的子结构。S3 引用 S0 的根结构、两次 S1：

```text
左 S1 的 [v0,v1] → S3 的 [v0,v1]
右 S1 的 [v0,v1] → S3 的 [v0,v2]
```

这两个接线映射明确要求两次 Mul 的第一个变量相同；不是仅比较三个算子存在。
相同变量名不是共享依据，alpha 重命名不改变模式；改变别名约束必须产生不同模式。
本版限于正向、无 guard/subsume 的 Math constructor rewrite，不声称是任意子图模式挖掘。

## 在哪些位置计算

分析阶段读取已饱和的两个参数化 egraph，统一完全相同的符号子闭包坐标。
随后对每条规则，在每个可达 e-class 锚点尝试匹配，包括 root 和非 root。
位置是 e-class 锚点，页面显示一条最短可达路径，不枚举所有展开树路径。

S3 实际出现三处：

| Cell | 位置 | binding（忽略 Add 的左右交换） |
| --- | --- | --- |
| T0 | root | v0=p0, v1=p1, v2=p2 |
| T1 | 内部 T0 子闭包 | 与上一行相同 |
| T1 | root | v0=p0, v1=p1+p2, v2=p3 |

T0/root 和 T1/non-root 共用同一份已计算结果。T1/root 只复用模式与接线程序，
结果 binding 不同，需要计算。不能仅因模式相同就共享结果。

## 真正计算匹配的矩阵门

已知父 constructor 行后，孩子的匹配与父行通过一个完整元组连接：

```text
(parent.arg_position, 已确定的共享变量值...)
(child.root,          对应的子模式变量值...)
```

左右元组编码到相同坐标，构成 one-hot 矩阵 L、R；`H = L Rᵀ` 为精确的配对门。
H 的非零位置决定哪些父、子 binding 可以合并。共享变量必须在同一个元组中满足，
不能将不同候选的半个条件拼成结果。参数位置和输出变量次序均保留。

例：S3 的第二个 Mul 同时检查：

```text
parent.arg1 = child.root
parent.v0   = child.v0
```

实际新边界上得到的一个门为：

```text
H = [1 0
     0 0]
```

虽然两边都有两个 Mul 表示，只有同一 p0 对齐的那一对合法。

坐标构造、键元组收集、输出 binding 构造仍是 CPU 工作，不能由点积吞吐推断端到端性能。
这里的矩阵原语是可核对的参考实现，不是免去了所有索引或稀疏 Join 工作。

## 统一矩阵的职责

另外生成 `U = A B`：

- A：每个 `(Cell, 锚点, 原规则)` 请求对应哪个共享求值；
- B：该共享求值对应哪些完整匹配 binding；
- U：全部请求的匹配结果。

本例 U 为 **72×26**。含 Cell/位置的结果共 **40** 条，实际不同结果 **26** 条。
U 用于展示共享与结果分发，不是产生匹配的运算；产生匹配的是前面的 H 门。
报告生成时物化 U，不把展示矩阵的稠密运算计入下面的 matcher 核心计数。

## 同算法消融：实际执行次数

三个变体完全相同，仅改变缓存范围；逐条输出一致：

| 实际计数 | 不缓存 | Cell 内共享 | 跨 Cell 共享 |
| --- | ---: | ---: | ---: |
| 模式／锚点求值 | 142 | 72 | 44 |
| constructor 索引读取次数 | 106 | 36 | 22 |
| 实际取出的 constructor 行 | 86 | 28 | 18 |
| 矩阵连接调用 | 9 | 9 | 6 |
| 构建的连接键元组 | 54 | 54 | 36 |
| 矩阵标量乘加（含零项） | 360 | 360 | 240 |
| 计算产生的 binding 行，含中间结果 | 98 | 40 | 26 |

含空锚点的查询也在求值次数里；行数单独统计，以免把消除空查询夸大成有效工作减少。
缓存查找、内存管理、矩阵坐标准备及报告输出不等于免费。本实验不是与 native Free Join
比较探针或计时。准备阶段另有 2 次 native Bake、35 行原生 constructor 数据读取、
18 行匹配索引构造，记录在 `report.json/preparation`。

## 新边界继承已有匹配结果

新增一个 coarse fact：

```text
Add(root(T0), root(T1))
```

在原有缓存上加一个新 Add 锚点，不改动旧事实：

| 新边界的工作 | 从头匹配 | 复用已有结果 |
| --- | ---: | ---: |
| constructor 行读取 | 6 | 1 |
| 矩阵连接调用 | 2 | 2 |
| 矩阵标量乘加 | 14 | 14 |

结果完全相同并与 native 核对。这里没有省掉新增的接线检查，只省掉已知内部匹配。
本缓存只在不可变闭包和新增独立锚点的实验条件下有效；动态 union/set/subsume 必须有
失效或版本机制，当前没有实现，不能直接接到任意变化的运行时。

## 验证

原生 oracle 查询使用原始 parser 导出的 LHS 和原变量名，结果字段按明确的变量顺序输出。
原来的 rewrite 先 saturation，新增 probe ruleset 再收集完整 `(anchor, binding...)`；
逐模板、逐模式、逐锚点比较，不只对数量。

- 所有四条规则、所有 root/non-root 位置、全部完整 binding 一致。
- 新边界的完整 binding 另与 native 比较。
- native 负例 `Add(Mul(p0,p1), Mul(p2,p3))`：同样有 Add 和两个 Mul，不能提取公因子。
- 合成负例覆盖错误共享变量、不同候选 binding 混合、左右参数交换和单节点重复变量。
- 关闭完整 Spine 枚举 API，整个实验仍能完成。
- 原有参数化因子化实验的 8 项回归也保持通过；共 16 项测试。

probe 的输出是逻辑 LHS 匹配记录，不是已提交 rewrite mutation。输出数量不是存储压缩率。

## 复现

```sh
python3 experiments/cell_spine/shared_matching/demo.py \
  --output out/shared-matching-next

# 已重新编译 lhs_ast 与 native_math_baseline 后，可加 --skip-build。
python3 -m unittest \
  experiments.cell_spine.shared_matching.test_demo \
  experiments.cell_spine.factorized_boundary.test_demo -v
```

输出目录必须不存在。默认离线构建，Python 使用标准库。
页面提供模式字典、binding 接线、root/non-root 出现表、三种消融、统一矩阵、实际连接门。
`probe.egg` 为自动生成、可独立运行的原生查询；`rules-ast.json` 为原 parser 的输出。
`results.json` 保存摘要；详细矩阵和证据在生成目录，不提交编译产物。

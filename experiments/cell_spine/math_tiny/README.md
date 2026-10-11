# Math microbench 超小子集：同一闭包，两个历史起点

规则逐字取自 `egglog/tests/math-microbenchmark.egg` 第 19、20、33、34 行：

```egg
(rewrite (Add a b) (Add b a))
(rewrite (Mul a b) (Mul b a))
(rewrite (Mul a (Add b c)) (Add (Mul a b) (Mul a c)))
(rewrite (Add (Mul a b) (Mul a c)) (Mul a (Add b c)))
```

只保留 Add、Mul、Var 类型，用一个输入替代完整 microbench 的输入集合，
将 `run 11` 改为这个有限子集的 saturation。两个输入分别是：

- `source.egg`：`x × (y + z)`。
- `expanded.egg`：`y × x + z × x`，乘法方向特意反过来。

## 实测

实际运行本仓库的 native egglog。没有修改执行引擎。不是 pristine 上游性能对比。
编译强制 `--mode symbolic`，不允许退回当前输入的阶段快照缓存。

| 指标 | x × (y + z) | y × x + z × x |
|---|---:|---:|
| 原始饱和 Math e-node | 13 | 13 |
| 原始 Math e-class | 7 | 7 |
| 压缩执行的 spine e-node | 5 | 6 |
| 压缩执行的 spine e-class | 5 | 6 |
| Summary 行 | 3 | 4 |
| 原生总表行：原版 → 压缩执行 | 14 → 9 | 14 → 11 |
| 原始规则逻辑匹配 | 14 | 17 |
| Spine-Run 逻辑匹配 | 3 | 5 |
| 实际匹配的多父转移 | 0 | 1（三个父实例） |
| 选中的 Cell 族 | 3 | 4 |
| 生成的 spine 规则 | 3 | 6 |
| 完整 library.egg 文本 | 4,083 B | 6,969 B |
| 显式导出后的 Math e-node / e-class | 13 / 7 | 13 / 7 |

原生总行数含命名 root 的绑定行。Summary 行和 constructor 行的列数不同，
这不是字节数/RSS 压缩率。逻辑匹配不是已提交变化或 Join 探针数量；宏规则与
原始规则的单次匹配工作也不同，14→3 不能当作加速倍数。这些匹配计数只统计
执行阶段，排除 Bake、Gurobi、显式闭包导出和对照验证。

完整闭包可以这样理解（每个花括号是同一 e-class 的不同表示）：

```text
root = { x*s, s*x, p+q, q+p }
   s = { y+z, z+y }
   p = { x*y, y*x }
   q = { x*z, z*x }
     + x、y、z 三个 Var
```

所以是 4+2+2+2+3=13 个节点、7 个类。因式输入的 spine 只存
`x、y、z、Add(y,z)、Mul(x,Add(y,z))`，加上 3 条 Summary。
完整的其他表示从缓存闭包导出。

## 哪条多父转移真正执行了

展开输入实际执行的 `impl_4_2`（变量已为阅读重命名）：

```egg
(rule ((Spine-Summary-0 addRoot left right)
       (Spine-Summary-1 mulRoot1 a x)
       (Spine-Summary-1 mulRoot2 b x)
       (= left mulRoot1)
       (= right mulRoot2))
      ((union left mulRoot1)
       (union right mulRoot2)
       (Spine-Summary-3 addRoot mulRoot1 mulRoot2 left right a x b))
      :ruleset Spine-Run)
```

对应 `a=y, b=z`。通过两个等价约束，把 Add 的两个参数与两个 Mul Cell 的
root 对齐。没有重新查询两棵 Mul 子树。其完整源码保存在
`expanded-generated.egg`；这是编译器自动生成的，不是手写替代规则。

但完整 coarse 模式的 `impl_7_1` 也匹配了一次，并指向同一目标族。
`Spine-Summary-3` 最终只有一条行。两条已生成的规则 `impl_3_2`、`impl_5_1`
则完全没有匹配；其中两父版还要求存在交换后的 Mul 结构，当前 spine 没有
显式存下这些结构。**本测试没有删掉这些不利结果。**

因此这个例子同时说明：确实省掉了一些内部结构与原始匹配；但替代路径仍会
重复匹配，spine 的紧凑程度仍依赖输入历史，父 Summary 的接口还不够完善。

## 验证和复现

6+8 条 Bake 路径与两个完整输入逐一对照通过；另外针对两个最终状态做了交叉
对照。比较包含所有 constructor 行及完整等价分区，不只是最后一个 `check`。
导出前原始 Math 表始终为空，原规则没有在压缩执行中暗中运行。

```sh
out/cell-spine-venv/bin/python experiments/cell_spine/math_tiny/run.py \
  --output out/math-tiny-demo
```

需要 Gurobi Python 环境和 Graphviz 的 `dot`。已有当前 native 工具时可以加 `--skip-build`。输出目录必须为空。生成：

- `index.html`：节点数量、每条生成规则的实际匹配次数、竖向原图/spine 图。
- `original.dot/.svg`、`factored-spine.dot/.svg`、`expanded-spine.dot/.svg`。
- 两个子目录的 `runtime.egg`、`restore.egg`、`selection.lp`、`transitions.json`。
- `results.json`、`detail.json` 和跨输入的 `cross-validation.json`。

已保存本次 `results.json` 和两份 `*-generated.egg` 供直接阅读。
新增回归测试：

```sh
out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_math_tiny -v
```

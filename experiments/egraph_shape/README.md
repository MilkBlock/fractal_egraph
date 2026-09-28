# e-graph 形状：level × log10(#eclasses) × e-nodes/class 分布

按用户提出的三维口径画 e-graph 的形状：**x = level**（每个 e-class 的层级）、
**y（深度）= log10(该层 e-class 数)**、**z（竖直）= 每个 e-class 的 e-node 数（log2）**，
每个 level 上画一条 **violin**（该层 e-class 的 e-node 数分布），脊柱连线是各层 e-class 数。

## 一个设计上的说明

x 取 level 之后，"该层有多少 e-class" 就已经是 x 的函数，不可能同时当作独立的 y。
这里的处理是：把 log10(#eclasses) 当作**深度**（violin 摆放的远近），把竖直方向留给
层内真正有方差的东西 —— e-node 数/class。等价的信息也可以画成 (level, e-node 数) 的
二维热力图，`results` JSON 里有全部数据，想换画法直接改 `shape.py`。

## 两种 level 定义

* `root distance`：从没有任何父节点的 e-class（root）出发的最短构造器步数，多 root 取最小值。
  这是用户原始提法。
* `leaf height`：离叶子的最短项高（0 = 该 class 里存在没有 Math 参数的 e-node）。对偶口径。

## 怎么跑

```sh
# 图 1：vanilla egglog AC n=10、n=12，加 math mini3 的最终 egraph
python3 experiments/egraph_shape/shape.py \
  --graph out/egraph-shape/vanilla-ac-n10.json:"vanilla egglog AC  n=10" \
  --graph out/egraph-shape/vanilla-ac-n12.json:"vanilla egglog AC  n=12" \
  --graph out/cell-evolution-complete/math-observe/verification-graph.json:"math mini3, 6 rounds" \
  --out out/egraph-shape/shape.svg --data out/egraph-shape/shape.json

# 图 2：79 个已导出的 ripen cell 内部形状（pooled）
python3 experiments/egraph_shape/shape.py \
  --cells out/aggregate-effects-direct-2/saturated-rule-composition/cells \
  --out out/egraph-shape/cells.svg --data out/egraph-shape/cells.json
```

AC 两个图需要先生成输入（约 3.5 MB / 33 MB，不入库）：

```sh
python3 -c "
import sys, subprocess, tempfile, pathlib
sys.path.insert(0, 'experiments/shortcut_rules'); import shortcut_bench as sb
for n in (10, 12):
    with tempfile.TemporaryDirectory() as td:
        egg = pathlib.Path(td) / 'p.egg'; g = pathlib.Path(td) / 'g.json'
        egg.write_text(sb.program(n, 'generic'))
        subprocess.run([str(sb.HARNESS), str(egg), str(g)], check=True)
        pathlib.Path(f'out/egraph-shape/vanilla-ac-n{n}.json').write_text(g.read_text())
"
```

输入格式是仓库通用的 constructor-row 导出：`[{"op","args":[["Math",v],...],"result"}]`，
以及 `SaturatedRuleComposition` 的 `values` + 带索引 `rows`（`--cells` 会自动转换）。
AC 图的来源是 `experiments/shortcut_rules/shortcut_bench.py` 的 pristine harness；
math 图是 cell-evolution 观测运行导出的最终 egraph（`verification-graph.json`，2,167 e-node / 748 e-class）。

## 读出来的四件事

**1. 对 AC 来说 root distance 是退化轴。** n=10：**1** 个 root class 自己装 **1,022** 个
e-node，其余 **1,022** 个 class 全部在 distance 1。n=12：root 装 **4,094**，其余 4,094 个
也在 distance 1。整个 AC 闭包是一个两层星形，按"离 root 的距离"分层没有信息量。
permutation/association 型饱和要用对偶的 **leaf height**。

**2. leaf height 直接把冗余律画出来了。** AC n=10 各层（class 数 / e-node 数/class 中位→最大）：

| leaf height | #eclasses | e-nodes/class |
|---:|---:|---|
| 0 | 10 | 1 |
| 1 | 45 | 2 |
| 2 | 330 | 14（6–14） |
| 3 | 627 | 62（30–254） |
| 4 | 11 | 510（510–1022） |

n=12 同构但更大：12 / 66 / 715 / 3,003 / 299，顶层 299 个 class 每个 510–4,094 个 e-node。
规律是：`leaf height = ceil(log2 k)`（k = 该 class 的叶子数），`#eclasses = Σ C(n,k)`，
`e-nodes/class = 2^k − 2`。所以**类的数量集中在中间层，冗余集中在顶层** —— 顶层是典型的
"fat head"：class 很少，每个都巨大（4,094 个 e-node）。这正是前面测到的 40× 重复求值
和 merge 碰撞在形状上的样子。

**3. math mini3 是健康的。** 748 class / 2,167 e-node / 7 个 root：root distance 各层
class 数 7,106,135,113,148,129,87,18,5（峰在 4）；leaf height 各层 8,26,82,146,167,161,91,44,20,3
（峰在 4）；**每 class 的 e-node 数中位 1–3，最大 46**（leaf height 3）。两个轴都是钟形，
没有 fat head。和 AC 的 1,022 / 4,094 差两个数量级。

**4. ripen cell 内部很小很浅。** 79 个导出的 cell：e-node 中位 **15**、最大 121；
e-class 3–42；leaf height ≤ 4；pooled 后每 class 的 e-node 数 1–15。也就是说
math 那个例子里的"胖"不在 cell 内部，而在 cell 之间的交互（前面测到 2,631 个 boundary
event、826,555 条 loose contact）。

## 边界

* root distance 需要能定义 root：多 root 时取到任一 root 的最小距离（AC 1 个、math 7 个）。
  有环的 class 拿不到有限 leaf height，本数据集里为 0 个。
* 图 1 的 AC 数据来自 vanilla egglog（baseline 标签），math 数据来自观测运行的最终 egraph；
  图 2 的 cell 来自另一份更早的 CSUnit 导出。**这是形状诊断，不是受控对比。**
* 三张图都是最终状态的静态形状，不显示时间演化；每层只是横截面。

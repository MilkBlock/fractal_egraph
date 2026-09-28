# e-graph 形状：level × log2(e-nodes/class) × log10(#eclasses)

按讨论后的口径画成**二维热力图 + 边缘分布**（不再是叠在一起的 violin）：

* **x = level**：每个 e-class 的层级，两种定义见下。
* **y = log2(e-nodes/class)** 的整数 bin（bin `b` 表示 `2^b ≤ n < 2^(b+1)`）。
* **颜色 = log10(该 (level, size) bin 里的 e-class 数)**，并在格子里写出原始计数。
* **顶部边缘 = log10(该层的 e-class 数)** —— 这正是最初提法里的"y 轴 log 之后的 eclass 数量"。
  它数学上是 x 的函数，所以放进边缘而不是当成独立坐标轴。
* **右侧边缘 = 该 size bin 的合并分布**。

`--mode violin` 保留上一版按层画分布的形状（叠在一起的那个），默认已是 `heatmap`。

## 两种 level 定义

* `root distance`：从没有任何父节点的 e-class（root）出发的最短构造器步数，多 root 取最小值。
* `leaf height`：离叶子的最短项高（0 = 该 class 里存在没有 Math 参数的 e-node）。对偶口径。

## 怎么跑

```sh
# AC 的输入先导出（约 3.5 MB / 33 MB，不入库）
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

# 图 1：vanilla AC n=10、n=12 + math mini3 的最终 egraph
python3 experiments/egraph_shape/shape.py \
  --graph out/egraph-shape/vanilla-ac-n10.json:"vanilla egglog AC  n=10" \
  --graph out/egraph-shape/vanilla-ac-n12.json:"vanilla egglog AC  n=12" \
  --graph out/cell-evolution-complete/math-observe/verification-graph.json:"math mini3, 6 rounds" \
  --out out/egraph-shape/shape.svg --data out/egraph-shape/shape.json

# 图 2：79 个已导出的 ripen cell 内部（pooled）
python3 experiments/egraph_shape/shape.py \
  --cells out/aggregate-effects-direct-2/saturated-rule-composition/cells \
  --out out/egraph-shape/cells.svg --data out/egraph-shape/cells.json
```

输入格式是仓库通用的 constructor-row 导出
（`[{"op","args":[["Math",v],...],"result"}]`），以及 `SaturatedRuleComposition` 的
`values` + 带索引 `rows`（`--cells` 自动转换）。AC 图来自 `experiments/shortcut_rules`
的 pristine harness；math 图来自 cell-evolution 观测运行导出的最终 egraph
（2,167 e-node / 748 e-class，7 个 root）。

## 从热力图上读出来的四件事

**1. 对 AC 来说 root distance 是退化轴。** n=10 只有两列：level 0 一列装着 1 个 class、
1,022 个 e-node；level 1 一列装着 1,022 个 class。n=12 是 1 / 4,094。热力图上就是
"一列孤零零的高格 + 一列宽色带"，整个闭包是两层星形。permutation/association 型饱和
要用对偶的 **leaf height**。

**2. leaf height 的热力图直接把冗余律画成一条上升对角线。** AC n=10：

| leaf height | #eclasses | e-nodes/class（min/中位/max） |
|---:|---:|---|
| 0 | 10 | 1 / 1 / 1 |
| 1 | 45 | 2 / 2 / 2 |
| 2 | 330 | 6 / 14 / 14 |
| 3 | **627** | 30 / 62 / 254 |
| 4 | **11** | 510 / 510 / 1,022 |

n=12 同构放大：12 / 66 / 715 / **3,003** / **299**，顶层每格 510–4,094。
闭式：`leaf height = ceil(log2 k)`（k = 叶子数）、`#eclasses = Σ C(n,k)`、
`e-nodes/class = 2^k − 2`。图上就是**类数集中在中间层、冗余集中在顶层**：顶层只有 11 个
class，每个装上千个 e-node —— 这就是前面测到的 40× 重复求值和 merge 碰撞在形状上的样子。

**3. math mini3 是健康的钟形。** 748 class / 2,167 e-node / 7 root：root distance 各层
7,106,135,113,**148**,129,87,18,5；leaf height 各层 8,26,82,146,**167**,161,91,44,20,3；
**每 class 的 e-node 数中位 1–3、最大 46**。热力图上是中段一条对角线带，
和 AC 的 1,022 / 4,094 差两个数量级。

**4. ripen cell 内部很小很浅。** 79 个导出的 cell pooled：79 个 root、770 个 class、
1,852 个 e-node；e-node 中位 15、最大 121；leaf height ≤ 4；size bin 集中在 1–8，
最大 bin 是 8–16（15 个 class）。math 里的"胖"不在 cell 内部，而在 cell 之间的交互
（2,631 个 boundary event、826,555 条 loose contact）。

## 边界

* root distance 需要能定义 root：多 root 时取到任一 root 的最小距离（AC 1 个、math 7 个、
  cell pooled 79 个）。有环的 class 拿不到有限 leaf height，本数据集里 0 个。
* 图 1 的 AC 数据来自 vanilla egglog（baseline 标签），math 数据来自观测运行的最终 egraph；
  图 2 的 cell 来自更早的 CSUnit 导出。**这是形状诊断，不是受控对比。**
* 都是最终状态的静态形状，没有时间演化；每层只是横截面。
* 热力图只按 (level, log2 e-nodes) 分箱，箱宽为 1 个 log2 单位；想换分箱或换成
  `--mode violin` 直接改 CLI。

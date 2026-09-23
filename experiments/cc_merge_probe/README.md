# 放开 saturated CS 之间的 CC 合并：单次小实验

实际使用本地原生 egglog，不使用模拟 matcher。此实验独立运行，尚未改变生产 CS 发现流程。

## 范围

`demo.egg` 只有加法交换律、乘法交换律和分配律。先将 `Add(p0,p1)` 与 `Mul(p0,p1)` 分别 saturation，得到两个已饱和 CS；每个有两个参数、三个 Math e-class 和两个算术 enode。

不要求 tier0 曾出现连接它们的 consumer。穷举四种有向配对（包括同族两个实例），并将右侧两个参数映射到左侧任意一个 e-class 或新外部参数。允许两个参数绑定同一 e-class，也允许新参数之间相同；包括不相交合并。

每对共有 17 种 binding，共 68 种合并。没有 Top-K、抽样或 ripen 轮数上限。但这是明确的**一次合并、两个饱和族**实验，不是任意深度的闭包枚举，也不强制 union 两个任意内部节点。

## 结果

- 68 / 68 饱和完成。
- 60 种至少一个参数连接到左侧图；8 种没有这种连接。
- 7 种在导入两个已饱和结果后继续产生新结构，归入 4 个不同结果族。
- 61 种没有进一步变化；这表示原生完整状态相同，不只是节点数量相同。
- 全部合并结果按固定参数、两个结果位置的接口精确判等，得到 44 族。44 包含简单并置结果，不代表 44 种有用的新规律。
- 最多 17 个算术 enode；最多 5 个 sweep，包含最后一次确认不动点的 sweep。
- 所有 68 个结果还与“从原始两个 coarse facts 开始 saturation”的结果精确判等，全部通过。
- 一次本地 release 执行约 **0.083 秒**，包括上述对照和判等；不含编译，不是完整 math-microbenchmark 或 bake 加速比。

原始结果见 `result.json`。计数不包含 Input 脚手架和 let；Input 对应的参数身份保留在判等接口中。

## 哪些连接真的有交互

只有左侧 `Sat(Add(p0,p1))`、右侧 `Sat(Mul(x,y))` 的配对产生新增结构，并且至少一个乘法参数接到左侧 Add 的结果：

```text
已有：g = Add(p0,p1)，以及 Add(p1,p0)
接入：Mul(g,p2)，以及 Mul(p2,g)
继续 saturation：启用分配律
```

该例算术 enode 从 4 增至 10。两个乘法参数都接到 g 时，即 `Mul(g,g)`，从 3 增至 17。

只共享叶子参数、或完全不相交的合并，在这个 demo 中没有进一步推导。已饱和图的复用确实让大部分尝试很快停止。

## 结论与限制

值得继续尝试放开**饱和族之间**的合并，而非直接枚举全部历史实例。本例支持“多数合并没有新的跨组合交互”的判断，但没有证明 saturation 会自动缩小接口。

即使每个右侧组合只有两个参数，左侧有 n 个可绑定 e-class 时，包含新参数及别名的映射数也是 `n² + 2n + 2`。更多参数和递归合并仍可能膨胀；目前没有测试第二代合并或完整 math 的饱和族。

## 复现

```sh
cargo run --release --offline --example cc_merge_probe -- out/cc-merge-new
```

目录必须不存在。若修改 demo 导致两个初始饱和族不再仅有两个交换行，实验会拒绝继续，避免把不完整状态冒充 Sat(C₂)。

## 可选的通用递归实验功能

默认 `analyze` 不变。通过独立的 `cc-merge` 命令启用：

```sh
cargo run --release --offline -- cc-merge \
  --rules experiments/cc_merge_probe/demo.egg \
  --seeds experiments/cc_merge_probe/seeds.json \
  --output out/cc-generation-1 --generations 1
```

`--rules` 是自包含的 datatype 与规则文件；`--seeds` 是 coarse facts 的配置。程序先对每个配置做 saturation，再放开已饱和族之间的配对。

```json
[
  {"parameters":2,"facts":["(Add p0 p1)"],"result":"(Add p0 p1)"},
  {"parameters":2,"facts":["(Mul p0 p1)"],"result":"(Mul p0 p1)"}
]
```

修改参数数目、facts、constructor 的参数数量和规则，就能测试不同规模。p0、p1 等是显式的外部参数；事实可以是 constructor 表达式或 union。`result` 指明该组合提供的结果。此命令从这些配置建立饱和族，目前不是直接导入旧 catalog 的接口。

- 第 1 代穷举初始饱和族的所有有向配对和 binding。
- 第 2 代把新发现的族也纳入配对；以此类推。已处理过的有向族对不重复运行。
- 一代开始时固定本代族集合，本代新族下一代才接续。
- 每个参数可以连接左图的任意同类型 e-class，也可引用新外部参数；穷举参数别名。流式枚举，不预先分配全部 binding 数组。
- 每个合并都继续原生 saturation，随后做完整状态判等。接口保留外部参数位置和当前 result；不保留每一步的历史 result 名字。因此其族编号/数量不应直接与旧的固定双-result demo 对照。
- `--generations` 是用户指定的实验范围，默认 1；不是自动发现预算。范围内没有 Top-K、binding 数上限或 saturation 限时。若规则无限生成，某次合并会持续运行，可主动终止。

输出：

| 文件 | 内容 |
|---|---|
| `generations.csv` | 每代配对数、binding 数、新族数、继续产生变化的合并数、最大参数/目标 e-class 数、耗时 |
| `merges.jsonl` | 每次合并的左右族、binding、fresh 参数数、结果族、sweep 数和事实数 |
| `families/*.json` | 完整饱和结构与固定接口 |
| `summary.json` | 已完成代的汇总 |
| `progress.json` | 当前 seed / 合并；正常完成或错误状态 |
| `rules.egg`、`seeds.json` | 本次输入副本 |

binding 中小于 n 的数表示左图第几个同类型 e-class（按其 `values` 数组筛选顺序）；n 及以后的数表示依首次使用顺序分配的新参数。同一编号表示 alias。族图导入保留循环与共享，内部使用私有 constructor 分配节点；统计图排除这些脚手架行，但耗时包含分配工作。

当前范围是单个等价 datatype、constructor/union 规则；constructor 的字段可为该 datatype 或 i64/String/bool。关系、一般 function、delete/subsume、include 和多 datatype 尚不支持。不会把不支持的程序当作已成功实验。

已验证：原 demo 的 68 个 binding 与 7 个继续变化的合并；小型两代递归接续且无重复配对；scalar 字段与循环图导入。尚未运行完整 math 的多代实验，也尚未证明增长率是指数级。

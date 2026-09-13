# Tier-2：稳定 relative binding 的规则幂

核心表示为 `HigherRule(k, R, ctx, binding)`。
`R` 是 `Extension`：源规则、relative-binding 更新的源 AST 端口角色、前提及 effect schema。
`ctx` 保留启动和 coarse 注入的历史；`binding` 是首次稳定应用的接口；`k` 计算从 ctx 开始的应用次数。
**不是把坐标的 Power 冒充 rule-combination 的幂。** 两者都有独立实现。

## 实际实现

1. `mine.py` 从已保存的真实 Math native tier-1 图和接口表提取局部扩展。
   把 ParentPort 的数值槽解析成“父序号 + 生产规则 + 输出 AST 角色 + 类型”。
   外部输入、变量别名、源规则 guard 和 union 保留在扩展键里；不合并语义未知的扩展。
2. `ir.egg` 是 native tier-2：观察同一扩展、同一父槽的重复路径，以及需要边界支撑的依赖边。
   事件编号须按拓扑递增；规则中也检查递增，以免坏输入导致重复长度无限增长。
3. `higher.py` 只把已有的、单父、完全 local 的 SmoothComb 边交给 native `higher_ir.egg`。
   `power-start` / `power-extend` 计算次数，`power-view` 生成 `HigherRule`，并用 `Represents` 关联原 Comb。
   具体次数不是规则硬编码的常量。所有前缀必须来自已存在的 tier-1 图。
4. `check_higher.py` 独立展开每个有限幂，确认抵达完全相同的原 tier-1 目标。
   这不是把原 e-class 自动 union 到新类型，也不是证明任意 k 的 effect 等价。

例如真实结果：

```text
HigherRule(4, ext_0067, comb_0063, initial_binding)
  represents comb_1196
```

`ext_0067` 是 R23 分部积分的稳定接口更新。起点 `comb_0063` 已含启动历史，4 只数之后的稳定扩展。
初始 binding 的完整 egglog 表达式在 `higher_native.json` 和页面中。

## 当前结果

- Math：1,529 次应用，288 种严格扩展描述，2,087 条依赖边。
- 深度 ≤3 为发现集；深度 >3 的 1,279 次应用中，646 次复用已见描述。
  这是同一工作负载的深度留出，不是独立数据泛化或压缩率。
- native Repeat 有 19 条子路径，最长 4 次；其中长度 ≥3 的 5 条**可重叠子路径**都来自 R23。
- 原生折叠得到 **13 个规则级 HigherRule**，全部通过有限展开回原 Comb 的检查。
  路径数与幂数不同：共享模板会去重，且只折叠 closed unary smooth 边。
- 127 条依赖边的消费端需要 coarse 边界；仅视为注入候选，不宣称已经证明 trigger 条件。

## 递推与 trigger 样例

`fixtures/*.egg` 真正在 egglog 中展开 F。Edge 是样例额外记录的关系，不冒充内核 committed mutation trace。
每条边的端点等价通过 native check；再用 Add 结合律和常量折叠，验证整个有限路径的端点。

`recurrence_bridge.py` 使用一个**加法递推专用适配器**，把路径转换为 `(剩余参数, 累积常数)` 接口。
累积常数是由已检查的边导出的坐标，不是运行时原始 binding。
这些接口进入 `recurrence_tier1.egg`，每个 Binding 都由 native tier-1 检查；`tier2_observations` 从原生关系读回。
随后才拟合更新，而非直接在样例答案上声称 tier-1 学会了通式。

- unit：学到 (-1,+1)，留出 15/15，得到 m+a=n，因而有端点族 f(n)=f(m)+(n-m)。
- triple：学到 (-1,+3)，留出 15/15。
- stride：学到 (-2,+1)，留出 7/7；参数差必须满足可达性，不能任取 m。
- changing：前 5 步看似 (-1,+1)，后半改变规律；留出 7/15，native tier-2 找到 8 个反例。
- warmup：前 2 步有不同 effect，训练前缀中识别 trigger_depth=2，之后使用 (-1,+1)。
  留出成功仍不代表全局成立：源规则包含启动分支，所以全局系数检查仍拒绝。
  要证明这条分阶段规律，还需要证明稳定阶段的 guard 闭合。

`affine.py` 只从训练段选择启动切点，至少需要 3 个稳定训练样本；留出段不能反过来修改切点。
源 AST 使用 egglog parser。系数检查验证翻译模型的不变量 `a*dm+b*da=0`，并核对源递推的更新。
`Compose`、`Power` 的规则是通用平移算子代数；具体增量由观察学习，不硬编码 f 的步长。
这部分等式仅指**坐标变换**，不表示不同规则 effect 可交换或原图中间节点被删除。

## 尚未完成的泛化

目前是可运行的第一版 tier-2，不是完整的 fractal rule 自动发现器：

- Math 使用严格源角色匹配，尚无任意 term-context 更新函数的 anti-unification。
- R23 已有有限规则幂，但尚未证明对任意 k 可继续，也未建立它的符号归纳不变量。
- coarse 多父重复还没有变成参数化外部 effect 流；当前停止幂折叠，保留原图。
- 不根据有限重复直接启用任意 k 的 tier-0 shortcut；不引入未绑定的 m 去枚举无限展开。
- 数学端点族依赖整数算术、Add 结合律及每一步源 guard；原生 i64 还需避免溢出。
- 没有测量加速或整个 egraph 的存储压缩率。

## 重现与查看

```sh
python3 experiments/tier2/run.py
```

入口 `index.html`；原生规则在 `ir.egg` 和 `higher_ir.egg`；真实 Math 输入在 `math.egg` / `higher.egg`。
结果是 `math_native.json`、`higher_native.json`、`higher_validation.json`、`affine*.json`。
固定 tier-1 输入来自 `experiments/tier1_extract`，无需重跑大型 tier-0 profile，也不依赖未提交的 math_tier1 目录。

## 显式 Reduce 与 cost

`reduce_ir.egg` 定义 EndpointExpr / Accumulation 两个互递归 sort。
`Reduce(fold, count, terminal)` 是 EndpointExpr 的显式构造器，cost=100。
普通符号、调用、加减乘成本为 1，除法和幂为 2。使用原生 egglog 的树成本提取，
不是事后字符串替换；该成本只是偏好解析表达式，并不保证任意巨大闭式一定胜出。

支持的先验恒等式：

- AddConstant(c)：terminal + k*c。
- AddArithmetic(first,step)：terminal + k*first + k*(k-1)*step/2。
- MultiplyConstant(c)：terminal * c^k。
- 任意重复更新零次返回 terminal。

前提为精确标量算术、k 是非负整数，常量摘要中的参数相对于重复次数不变。
`ECount("k")` 明确表示非负整数参数；普通 `ESymbol("k")` 不具有此假设。
负次数、未知 OpaqueFold 和未证明非负的符号次数保留为 Reduce。
这里不使用机器整数去常量折叠符号多项式；它不是浮点、矩阵或任意非交换算子的恒等式。

`higher_ir.egg` 的 `ReductionInput(higher, fold, terminal)` 是调用者提供的累积摘要；
`higher-endpoint-reduce` 用 HigherRule 内的 k 生成 `EndpointView(higher, Reduce(...))`。
随后运行 `endpoint-reduce`，同一端点 e-class 才出现便宜的解析表示。
没有为真实 R23 擅自声明加法/等差摘要，也没有删除原 tier-0 或 tier-1 中间结构。

`reduce.json` 保存原生提取前后表达式和成本。常量累加 112→13，等差累加 114→29；
三个应保留的反例均仍含 Reduce。`tests/tier2_reduce.rs` 另用 81 个有限求和案例检查
等差闭式、检查原 Reduce 节点仍存在，以及 HigherRule 到 Reduce 的连接。

## Fractal 轨道视图

`fractal.html` 只渲染已观察到的稳定连续应用与其前置 trigger 上下文：

```text
trigger → 1 → 2 → 3 → 4 → …
```

`fractal_view.py` 沿实际 occurrence 的父依赖寻找最大链，不能仅因模板相同就把无关实例拼接。
现有 13 个 HigherRule 的所有有限前缀，都映射到 4 条最大链的连续片段；当前有 12 次应用、
16 个可见组合模板（包含 trigger）。这是对 fractal 区域的聚焦，**不是对全部 1,389 个模板的无损压缩**。

点击 trigger 查看生产规则、整组合结果及外部接口；点击数字查看该次应用的整组合结果，
并展示从源 AST 解析的数学公式与稳定 binding 更新。没有单条规则降级结果时明确显示原因。
trigger 表示已观察到的前置上下文，不是已证明的最小充分触发条件。
省略号只显示未验证延伸的说明，不生成第 5 次应用或虚构其结果。

仅渲染当前详情，页面无需服务器或外部脚本。URL 如 `fractal.html#chain_1/4` 可定位某一步。
`test_fractal_browser.cjs` 使用 Playwright 和本机 Chrome 验证所有按钮、深链接、未知延伸、窄屏布局及脚本错误。
需要 Node 能找到 Playwright（如设置 NODE_PATH 到依赖目录）：

```sh
python3 experiments/tier2/fractal_view.py
node experiments/tier2/test_fractal_browser.cjs
```

## 选择重采集或复用

```sh
cargo run --release -- analyze --recapture-tier0 --source egglog/tests/math-microbenchmark.egg --rounds 11 --output out/math11
cargo run --release -- analyze --reuse-tier0 --output out/math11
```

不指定输出目录的复用模式仍使用仓库内的固定 6 轮快照。重采集默认 6 轮，显式 --rounds 按实际次数执行，
不会静默截断为旧快照的轮数。新目录必须不存在；输出保存独立的脚本、源程序、trace、tier-1 和 tier-2 结果。
因此原有浏览页面和固定快照不会被重采集覆盖。所有阶段成功后才写 `run.json: status=complete`，
失败写 failed；复用拒绝未完成的目录。

实测新采集 2 轮（64 个应用、42 个组合、0 个 HigherRule）和 6 轮（1,529 个应用、1,389 个组合、13 个 HigherRule）
均完成整条链路；复用 2 轮目录也完成。11 轮参数已支持，尚未完成 11 轮带 trace 的整链路资源验证。
轮数增大带来的 trace/导入成本没有被本次参数接口消除。

采集器及 compact tier-1 导出由 research 工程按需构建。`research/math_bridge.py` 是已有 Math 专用导入适配器，
不是新的组合器。其保守边界仍见输出的 import_audit；match 事件数不当作 committed mutation 数。

`--source PATH.egg` 指定实际输入；相对路径以仓库根目录为基准，绝对路径也可使用。
输入内容原样复制到独立运行目录，`run.json` 分别记录原路径、暂存路径和源文件 SHA-256。
生成规则的 Math datatype 从所选输入的原生 AST 提取，不再复用固定快照的声明。
`--source` 不可与复用模式搭配；当前仍是自包含 Math 程序适配器，不是任意 .egg 的通用导入器。

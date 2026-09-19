# Coarse/smooth layers

Tier1 的权威表示现在是 `src/coarse_smooth.rs::LayerStore`，不是 `.egg`。
原生 collector 的有效 apply DAG 直接增量进入它，在线、离线、history replay、
本地 debugger 共用这一入口。没有 prefix 或 block 成员资格门槛。
这没有扩大内核 trace 的物理见证覆盖范围：不完整或不受支持的 match 仍不能冒充有效 mutation。

## 术语迁移

| 旧术语 | 新位置及语义 |
|---|---|
| `Comb` | `Comb` 共享定义，规则源（含 guard/literal）、规范化父连接、binding、alias、effect 形状；不包含实际 event/value ID |
| `CoarseComb` | `CombKind::CoarseComb`；首次接入外部值/事实，或为跨层依赖显式重开接口 |
| `SmoothComb` | `CombKind::SmoothComb`；已声明依赖提供所需输入与事实，无额外注入 |
| `Empty` | 空父列表；种子 apply 统一作为 coarse 入口，不再建立 Empty 实例 |
| `ParentCombs` / `ParentAt` | `Apply.parents`；允许多个来源，保留汇合 DAG，不枚举组合子图 |
| `RelativeBinding` / `ParentPort` | 按输入顺序的稀疏接线，精确指向父 occurrence 的输出槽位 |
| `PartialRelativeBinding` / `External` | 同一个 Rust 枚举里的 External 槽位；无需再复制整套列表类型 |
| `Occurrence` | 具体 apply 及其证据，与共享 Comb 分开 |
| `RowFact` | 精确 typed token / row-version ID，union 不会将两条行事实合并 |
| `Equal` | 等价 effect，支持按需传递查询，不改变历史 producer 身份 |
| `Requires` / `Produced` | `Apply.required` / `produced` |
| `Provides` / `SupportsUse` | Rust 按父依赖查询证据；检查发生在当前 apply 的写入加入之前 |
| `ProposedRemoval` / `RedundantForUse` | 旧实验接口，不迁移为自动删除；覆盖不等于效果等价 |

原来的 `Make/MakePartial` 并未用于 native Record 的端口导入；任意 binding 表达式
仍由现有 binding DAG/reduce DSL 处理，本次没有实现第二套表达式解释器。
旧的泛型 `HasFact`/`ArgsEqual` 仅留作历史实验；当前 native 输入使用精确 RowFact。

## 层的构建

1. 验证端口、前向引用、事实支持。失败不插入部分节点。
2. 为 apply 的结构建立共享 Comb，参数值只留下 alias 形状。规则字面量保留在源规则中。
3. coarse apply 创建一个单成员 CoarseLayer，父 occurrence 作为入口依赖保留。
4. smooth apply 继承实际父依赖的 coarse support。
5. 只有一个 smooth apply 真实汇合多个 support，才创建联合 CoarseLayer；记录该 apply 为 witness。
6. 对同一 coarse support 的 smooth 成员收集到 SmoothLayer，成员之间仍保留依赖边。
7. 如果 support 内某个 coarse 成员依赖另一个成员，中间经过 smooth 或被遗漏的 coarse 成员，则不强行合层；当前 apply 重开 coarse 接口，保留其父依赖。

共享常量或相同 e-class 不会引发层合并。不同 coarse 成员各自有 smooth 消费者也不够，
必须有一个共同消费见证。每个成员至少一项贡献即可，不要求使用一个 rule 的所有输出。
这些判定依赖 caller 提供的真实 producer/read/union 依赖，不能用无证据的邻接关系代替。

联合层不删除子层；重复 support 复用同一个接口。当前使用带 children 引用的稀疏集合，
不强制语义上的成员数为 2^k。三个成员无需补一个无关成员，也不生成任意两层的笛卡尔积。
本次没有实现二进制 support trie 或按收益选择最优分区；巨大真实 support 仍可能较大。

## 与 Tier2 的边界

`src/layer_bridge.rs` 只有类型和关系声明，没有 Tier1 ruleset、rule 或 saturation。
Rust 验证完成后投影旧术语的 Comb/Occurrence/Binding/SupportsUse/Provides，供已有
FractalComb、Reduce 和 viewer 继续使用。跨层 restart 的 coarse 分类也传入此投影。

因此：旧 Tier1 `.egg` **解释器已退出主路径**，但是旧 Comb 形状仍作为下游兼容投影存在。
Tier2 的历史 unary recurrence 识别尚未改成对完整 CoarseLayer/SmoothLayer 模板进行归纳。
不能把本次结果称为新的 layer FractalRule 已经自动发现。
兼容投影为旧 FractalProvides 查询 materialize 继承 effect；Rust store 自身不复制每个节点的完整 effect 闭包。

`research/legacy_tier1.egg` 是移出的旧解释器，仍供显式的历史实验及差分回归测试使用。
旧 block/Zobrist 研究代码保留在 research 工程，不被本次主路径调用。

## 归一化边界

同一 source rule 下，不同具体 binding 可以共享定义；参数顺序、alias、effect 形状必须一致。
父引用按输入槽位首次使用排序，消除父列表顺序差异；未用于值端口的证明父节点也保留。
完整原规则是定义 key 的一部分，当前不将重命名后的不同源规则自动视为同一规则。
定义 ID 和层 ID 是运行内编号，不是跨运行的语义 hash。对照时比较递归展开的定义及事件成员映射。
不声称解决任意 DAG 同构、组合重分组等价或语义 dominance；这些仍是后续工作。

## 复现

```sh
cargo test --test coarse_smooth
cargo test --test native_history --test native_single_process --test bake
cargo run --release -- analyze --recapture-tier0 --save-history \
  --source egglog/tests/math-microbenchmark.egg --rounds 6 --output out/coarse-smooth-math6
cargo run --release -- analyze --replay-history out/coarse-smooth-math6/history.json \
  --output out/coarse-smooth-math6-replay
```

`layers.json` 列出共享定义、coarse/smooth 层、共同消费 witness 及 occurrence 之间的连接。
它不重复完整具体 value/effect 历史；完整调试证据使用可选 `history.json`。
本地 debugger 的 application 记录包含 `layer` 地址；不发布或重新构建线上网站。
测量必须区分真实 tier0 事件数、有效 apply 数、共享定义数、层接口数和归纳出的旧 Tier2 recurrence 数。

## Math 6 轮实测

最终实现：1529 个有效 apply，1405 个共享 Comb 定义，400 个 CoarseLayer 接口，
185 个 SmoothLayer，93 次为跨 smooth 边界而重开 coarse 接口。
CoarseLayer 成员数分布为 1:308、2:56、3:23、4:9、5:3、6:1。
直接相依的 coarse 成员可以同层；不得跨过中间 smooth 推导强行合层。
在线、离线及保存历史重放的 layers 完全相同；在线/离线解析后的 history 完全相同。
旧 Tier2 仍得到 13 个有限重复结果。定义数不等于字节压缩率，本次不声称存储或速度收益。
可复现命令和紧凑统计在 [coarse-smooth-math6.json](coarse-smooth-math6.json)。

## Layer 模板、FractalComb 与逐轮图

`src/layer_patterns.rs` 从 Rust LayerStore 提取有接口的有限模板，`src/native_layer_view.rs`
导出每个真实执行边界；渲染统一在 `tools/egglog_debugger` 现有网页。入口之外的祖先历史不进入模板 key；端口角色、共享/alias、
原规则 guard/literal、精确 RowFact 和 union effect 保留。变量使用每个成员自己的 Input 槽位。
源表达式通过 egglog parser 转为 `BindingExpr` / `Condition`；未知输出是 Projection，
不利用 constructor injectivity 猜测值，也不拟合样本数值。返回 binding 引用成员的输出槽位，
输出表达式仍是有作用域的符号对象，不等于已求得解析解。

候选来自三处：单步、停在 coarse 入口的有界依赖片段、实际返回同源规则接口的路径。
因此同一个 SmoothLayer 内的重复不会被漏掉；支持一对多返回，外部输入需求逐单元保留。
无关消费者保持开放，不强行纳入每一个递归体。layout 的 boundary_restart 本身不当作语义 turning point。
同类单元之间存在真实返回依赖，才报告 observed finite FractalComb；单纯重复出现不算递归。
`max_observed_depth` 是观察深度，不能解释成任意 n 的归纳证明。
多条路径可为同一入口提供不同展开选择；记录的是有限见证 DAG，不声称穷尽所有未来 apply。

覆盖复用原有有预算的非诱导 DAG embedding，检查绑定接线、全片段 alias、effect 标签，
允许较大图在映射根之上有额外节点。优先比较已发现 FractalComb 对应模板和高频模板。
`TemplateCoverage` 只证明 body 的结构覆盖；同时给出节点映射和真实实例重叠数。
边界条件蕴含、返回契约等价和任意深度的 `FractalDominance` 明确为 unknown，不能自动替代或 union。
第一个结构映射未通过 binding 检查时标记 UnknownBindingMapping，不谎称不存在其他有效映射。

为避免再次枚举所有子图，当前每个片段最多 32 个成员，forward return 单元最多 16 个成员、
3 跳/64 个访问点，每个入口最多 64 条直接返回候选、每张快照最多 16384 条递归见证连接；每张快照最多 512 次覆盖查询，单次 2000 搜索状态。
截断及未比较数随结果输出。这些是计算预算，不是规则语义，也不是完整性的保证。

```sh
cargo run --release -- analyze --recapture-tier0 --save-history \
  --source egglog/tests/math-microbenchmark.egg --rounds 6 --output out/layer-fractals-math
cargo run --release -- analyze --recapture-tier0 \
  --source experiments/bake/binary-1.egg --output out/layer-fractals-binary
# 离线和 --replay-history 同样生成逐轮图；新 history.json 保存轮次边界。
```

输出：

- tools 调试网页：`python3 tools/egglog_debugger/server.py --port 8080`。原有页面新增 Layer 面板，支持轮次、coarse/smooth layer、Fractal 模板选择。
- `rounds/round-0001.layers.dot`：本轮累计的 apply DAG 和 coarse 共享接口。
- `rounds/round-0001.fractals.dot`：FractalComb、trigger、有限单元及各返回端口。
- `rounds/round-0001.coverage.dot`：已证实的结构覆盖，箭头从大模板指向小模板。
- `rounds/round-0001.json`：快照（`analysis` 中含模板、所有有限见证、返回绑定和覆盖查询结果；另含 DOT、图数据和源码位置）。
- `rounds/manifest.json`：快照实际边界、对应轮次和计数。

即使某一轮无新增 apply，也单独生成该轮 DOT。在线模式在进入下一轮前写出快照；
离线/重放只用边界之前的前缀重建，不将最终图复制冒充早期状态。
不可拆开的复杂 schedule 标为 execution-boundary；没有边界字段的旧历史只输出
final-history-snapshot，不能倒推出当时每轮状态。CLI 结果可在 tools 页面“载入分析目录”中打开；网页运行时逐轮保存到 `out/debugger/<run-id>/rounds/`。
CLI 数据导出不依赖 Graphviz、网络或 Python 子进程；网页复用现有 `/api/render` 的 Graphviz 服务，以及 Eggplant 插件的 Fractal Typst 模板，不再提供第二个独立 HTML 渲染器。
如需 Graphviz 排版：`dot -Tsvg rounds/round-0006.fractals.dot -o fractals.svg`。
逐轮累计可视化会增加本地输出体积，不能把这一轮改动当作运行速度或总存储压缩证明。

网页的单成员单返回 Fractal 继续使用已有展开模板，展示 trigger、apply once/twice 和折叠深度。
多成员/多返回单元按成员复用同一插件公式渲染，展示成员接线与返回端口，不冒充单规则线性展开。
运行日志导出升级为 v3，携带 layer_snapshots；旧 v1/v2 日志仍能回放。
新 history 保存原始源码（包括显示注释），以便重放时继续使用原来的 Typst 配置；旧历史缺源码时使用规范化的声明/规则作为预览源码。

跨轮分析器缓存相同接口模板对的结构匹配结果；实际实例覆盖每轮重新计算，缓存不引入未来见证。
已有 tools 网页的 171 轮流式回归覆盖了此路径。目录加载与实时快照共用一份数据和插件渲染接口。

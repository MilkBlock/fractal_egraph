# eggfuse：组件边界与架构审计

状态：2026-10-29。本文保留架构审计及目标设计。第一阶段已落实公共 Recipe IR、编译器/Metal
源码归属和统一入口，见[当前组件与边界](../../eggfuse/README.md)。Rust crate 拆分、通用事件
订阅和更广的全局快路径仍未完成。审计中的性能数字来自已保存的完整 Pass-3 对照。

新增设计（2026-10-31，尚未实现）：[Cell 常驻推理运行时完整计划](cell-resident-runtime-plan.md)。
目标是以共享配方、实例状态和边界依赖替代热路径的全局事实表，先实现 CPU 语义核心再降低到 GPU。
该计划单列新的执行与验收契约，不改变本文记录的旧模式及历史结果。

新增状态：通用 region pass 和 GPU Spine 接线已实现，见
[通用区域执行记录](../../experiments/region_cells/README.md)。源码 marker 选择器和旧实验快路径的
下述审计保留。新路径已能通过完整键共享关联 binding、响应 table mutation，并复用仍有效的
fact 见证；尚未完成逐端口订阅、通用高吞吐并行执行或全量 Pass-3 判等修复。

## 结论

项目统一称 **eggfuse**。Analyzer 和 engine 都是它的组件；两者之间还需要一个职责明确的
**compiler**。现有系统已有 pass manager，但优化契约尚未贯穿这些 pass。

当前不是“analyzer 不会导出 union，GPU 不会执行 union”。两边已经具备相当广的语义支持。
缺口是：从实际闭包证据自动提取可复用的输入接口、参数更新和完整 effect，再编译成
避免重新搜索的运行时计划。现在最通用的路径仍执行局部查询，最快的路径则依赖特定模式。

目录整理可以让责任明确，但不能代替这个能力建设。

## 当前三条路径不能混为一谈

| 路径 | 实际能力 | 不代表什么 |
|---|---|---|
| Rust `cell_recipe` / `cell-device-recipes.json` | 保存环境、参数入口、程序、调度与初始化的独立 ripen 状态；有 native/device 执行与检查工具 | 不自动授权拿一个独立 Cell 的结果替换全局推理 |
| `generic_cell.py` / `.metal` / `.swift` | 支持所覆盖的 typed 构造、guard、函数更新、union、局部 rebuild 和控制流；可运行多个独立实例 | 仍做局部 Join；整个原程序作为一个实例运行，不是已经发现了高效的 Cell 分解 |
| `executable.py` → `joint_polyspine` / `cell_spines` / `cell_updates` | 验证参数化边界配方和更新程序，接管真实 terms 联合闭包 | 认证范围并不等于 generic interpreter 的语法支持范围 |

`src/cell_recipe.rs::register_device` 明确导出
`live_substitution_authorized: false`。它为独立 ripen 会话构造局部饱和 schedule；
`cell_schedule::device_entry` 另外保存原程序的真实控制树。这两种 schedule 不能互换。

[通用路径的现有限制](../../experiments/device_backend/GENERIC_CELL.md)包括：独立实例和全局
图之间的所有权／接口契约未完成、通用执行器仍搜索局部表、source-entry 模式不支持部分
控制命令。应保留这些边界，不把可序列化或可回放当成已经可以安全加速全局执行。

## Analyzer 的不足

### 1. 导出的是较完整的程序，却没有普遍导出便宜的入口

`cell_recipe.rs` 已编码条件、action 和 schedule，并共享语法 DAG。语法 DAG 共享减少
重复存储，却不决定一次输入变化后哪些读端口已经就绪、剩下什么需要查询。

需要补出的不是更多终态，而是：

- 完整关联 binding 的槽位及类型，保留实际 fact 见证和 alias 约束；
- 哪些条件已由 Cell 保证，哪些必须从外部取得，哪些仍需普通匹配；
- 哪个提交事件能够直接建立或更新入口；
- 输入或等价关系变化后，哪些操作重新执行，哪些结果继续有效；
- 每个输出如何供后继使用，而不先回表重新发现同一结构。

这些不能只由终态图或 rule 名称推断。

### 2. 参数化和 fast recipe 的覆盖没有统一

`eggfuse/compiler/recipes/executable.py::environment/derive` 直接限定三个 `terms` ruleset；
`closure_contract.py`、`update_recipe.py` 和 `joint_polyspine.py` 对实际 helpers、候选关系、
代价表与 merge 形状进行检查。检查是必要的，但目前主要是一种已知模式的专用编译器。

旧的参数族、闭包端口、spine 和 source marker 分析没有普遍汇合到这条可执行路径。
增加训练 Cell 数量不一定增加快路径覆盖；即使所有训练观察都导出，后端也可能没有对应
的优化 lowering。

### 3. Source marker 的搜索空间仍偏窄

`tools/cell_spine/pass3_markers.py` 目前提供共享 constructor 对和连续片段，后继要能由
已绑定字段完整查键。它保留全部关联变量，对可能 delete/subsume 的源表采取保守排除。

这足以支撑一个正确的正向视图编译器，却没有普遍覆盖：可变函数端口、反向多值 fiber、
带关联键的动态上下文，以及发布后可精确失效的 marker。当前完整 Pass-3 流程选出了
零个新增源码 marker，不能把 terms 内部端口复用说成源码 marker 已经覆盖了热点。

[Marker Program](../../eggfuse/compiler/marker_ir.py) 和
[native-row Interface](../../eggfuse/ir/interface.py) 都有价值，但当前消费者和
授权方式不同，需要收敛，而不是再造第三套来源不明的缓存。

### 4. 完整输入分析的成本还没有得到控制

上次全量 Analyze 在初始化历史阶段被 SIGKILL；原因尚未确定。成功的编译采用显式小型
训练输入，并核对它的规则环境。这是有边界的 Bake，不是完整 Pass-3 覆盖证明。

应把初始化事实的来源、可消费的运行时观察和需要保留的证据区分开，按真实事件增量
构造依赖；同时记录输入准备、历史保留、候选注册、ripen 和导出的成本。不能靠悄悄丢弃
记录或改变 schedule 来获得“可扩展”的数字。

### 5. 优化目标离实际执行成本还远

有 profile 接口，但没有完整的自动归因链。默认 source-marker 选择仍使用结构成本。
减少原子数、增加 Cell 数、提高节点覆盖率，都不保证降低端口维护、关联组合与提交成本。

需要按原阶段和消费者记录维护成本、实际 binding 数、残余搜索、失效／回退、共享次数及
必要 effect 数，再反馈给布局选择。未测项应为未知，不能补零。

## Engine 的不足

### 1. 快路径靠重新识别模式接通

`joint_polyspine.py::compile_contract`、`matrix_sat.py::analyze` 和
`scheduled_matrix.py::prepare` 分别拥有不同的准入逻辑。运行时并没有普遍消费 analyzer
导出的“条件已满足＋接线已确定＋后继动作”契约；相当一部分语义还要在 lowering 中重新推导。

Engine 应接收已经验证过的操作、接口和能力要求。符号名字可以作为数据保存，但不能
把 `terms`、`TCPair` 等名字作为架构层面的必要条件。名字无关也不等于可以省掉 typed proof。

### 2. 通用执行和高吞吐执行之间仍有很大空档

`generic_cell.lower` 复用现有 query/action 实现，在独立 arena 内解释执行；其通用路径
关闭并行匹配、并行准备和并行 rebuild，串行推进每个实例的规则。它是重要的语义参照，
不是已经把每个 recipe 编成大矩阵。

反过来，matrix fast path 主要接受固定载体上的一元性质传播，拒绝会逃逸的谓词返回身份、
很多 guard、结构增长和函数更新。两者之间需要多个有明确契约的优化 lowering：
参数运算、完整键查找、关联分组、结构构造、按键 merge、union block 等；不是所有操作都要
伪装成 GEMM，也不能把这些操作分成互不共享上下文的流水线。

### 3. 阶段版本已经正确区分，但重用粒度仍不够

三个 terms 阶段共用接口和端口投影，但仍有大量 private Run、任务准备、排序、patch、
版本维护与有序 RHS 提交。合并 encoder 不会消除这些实质工作。

应由 recipe 明确依赖和需要保留的版本，支持连续消费已计算的局部结果；外部消费者可见
之前才执行必要的发布。同时必须保留 constructor-producing merge 的中间事实、各次 set
的顺序要求、union 后新增的合法 binding，以及未覆盖规则的读取边界。

### 4. 全局接入契约尚不完整

独立 Cell 上的精确终态，不自动解决以下问题：实例重叠、跨 Cell 新匹配、全局 hashcons、
函数键合并、alias 专化、subsume 后旧端口失效。不能把这些问题藏在一个 `closed=true`
或“按 Saturate 执行所以都安全”的标志里。

某些交换/重排可由纯正向规则证明；自定义 merge、可观察中间构造和阶段选择需要更强的
约束。未知情况保留原执行路径，并单列回退成本。

### 5. 原生完整语义门槛还没有通过

当前完整 Raytrace 结果：source SK/MK 原生端点一致；GPU 与历史 GPU 端点一致；但 native
和 GPU 分别有 697,922 / 699,784 行。提取结果一致不能替代这个检查。

在查清差异机制之前，广泛采用新的合并顺序会让差异更难定位。优化准入应同时有独立
Cell 回归、区域边界对照、完整程序对照三个层次，不能只拿两个 GPU 版本互相比。

## 中间缺的统一契约

建议统一一份 **Recipe IR**，同一份 IR 可以由通用解释器执行，也可以逐步优化。
“有 recipe”与“该 recipe 已有高效执行计划”必须分开统计。已有三种产物通过适配器进入
它，不要求一次性删除历史格式。

| 部分 | 必须表达的内容 |
|---|---|
| 类型与接线 | 参数、局部身份、完整关系键、fact 见证、alias 和关联行 |
| 输入接口 | owner、输入端口、已保证条件、待查条件、就绪及版本信息 |
| 程序 | read/query、guard、整数运算、construct、set、union、可见性与错误行为 |
| 控制 | 原始 Run/Repeat/Saturate 的位置、循环进度、可见性边界 |
| 依赖 | 输出到后继输入的接线；变化和失效传播；merge 回调的传递读写集合 |
| 授权 | 观察证据、typed 验证、可复用范围、所需能力、未覆盖部分的执行方式 |
| 发布 | 局部到全局身份映射、有序 effect、去重、merge/rebuild、失败处理 |

代码已经拥有其中不少部分，工作是统一它们的含义和交接，而不是重新实现另一个解释器。
不要把最终表快照直接叫作 recipe；也不要要求每份 recipe 必须没有 query。

### 用真实 context 规则检验这个接口

对于原规则中 `(AddContext ctx (Bop op x y))` 的变换：

```text
入口关联行：AddContext(ctx, expr) = root
已取得的见证：Bop(op, x, y) = expr

a = construct AddContext(ctx, x)
b = construct AddContext(ctx, y)
set Bop(op, a, b) = root
```

`set Bop` 是 constructor 写入，已有键冲突时需要正确的等价处理。不能只生成最终表达式，
丢掉两个 AddContext 或 set 的可观察行为。

Analyzer/编译器应导出这个有类型的参数程序、完整输入见证、输出接线及其阶段限制。
Engine 实例化它；新 AddContext 可以成为后续合法 Run 的输入，而不用重新发现其来源。
但是入口只认识 AddContext 时，不能假装 Bop 见证已经取得。多 Bop 见证、迟到 union、
跨实例别名和间接 merge effect 必须落在契约内，否则继续查询/回退。

这段程序比三条专用 fast path 更通用，但**本文没有宣称它已获得全局批量闭包认证**。
它应成为下一条覆盖验收用例，而不是继续测 encoder 数量。

## 编译流程与 pass 边界

```text
source.egg
  → native parse/typecheck + 原始控制树
  → 结构键与明确选择策略
source_sk.egg
  → Analyze/Bake：历史、局部闭包、接口与族的证据
  → Recipe IR 构造与验证
  → spine / marker 布局、事件发布与消费者改写
source_sk_mk.egg + Marker Program + Recipe IR
  → 原始调度区域划分、typed effect / capability 验证
  → 共享子程序、端口接续、参数化与物理布局优化
  → CPU 参照 / 通用 GPU / 已认证的 GPU fast path
artifact manifest + Device Program + kernels
  → engine 执行 → 分层验证与成本反馈
```

源码 marker 是可检查的语义视图，GPU 上的矩阵／索引布局是其物理实现，两者不必一一同形。
一个优化 recipe 必须保留来源和降级方式；学习模型不能改变固定算术列或替代精确授权。

现有 `eggfuse/compiler/passes.py` 可继续使用。它已有依赖、指纹、产物校验、失败记录和缓存。
不足在于：边界主要是文件，有的下游直接导入上游实验实现；调度编码与停止语义有多份实现；
不少 pass 共享 `pipeline.py` 的整文件指纹，局部编排修改会失效过多缓存。

因此要采用 LLVM 式的 **pass 职责、IR 验证、analysis invalidation**，但暂不需要仿造庞大的
LLVM 插件框架。每个 pass 记录：输入 IR 版本、保持的语义性质、失效的分析、失败/回退原因、
输出检查与实际成本。性能选择和正确性准入是两条不同的判断。

## 建议的文件布局

下面是目标，当前真实实现路径仍列在后面的映射中。

```text
eggfuse/                         # 项目名称；无需强制改本机 checkout 路径
├── crates/
│   ├── eggfuse-ir/              # 共享契约；不依赖 Metal 或实验脚本
│   ├── eggfuse-analyzer/        # Rust 历史、ripen、Cell/族和证据
│   └── eggfuse-cli/             # 前端、analyze 命令、编译/运行入口
├── eggfuse/                    # Python 包
│   ├── compiler/               # passes、结构键、marker、recipe 优化
│   │   └── targets/metal/      # 物理 lowering、Metal 代码生成
│   └── ir/                     # Python 契约绑定与 verifier
├── engine/metal/               # Swift 调度/存储 + 通用 Metal 执行原语
├── validation/                 # native oracle、快照比较、真实 extraction
├── benchmarks/                 # 可复用的采集、对照、归因脚本
├── experiments/                # 原型、用例、消融和报告；生产代码不导入它
├── tools/egglog_debugger/       # 本地可视化
├── egglog/                     # 原生基准/本地 kernel 修改，独立记录来源
└── docs/architecture/
```

不必为了目录对称创建大量空 crate。先按依赖隔离模块，形成稳定 API 后再决定独立 crate。

| 现在 | 目标职责 |
|---|---|
| `src/native_*`、`src/cell_recipe*`、`src/cell_schedule.rs` | analyzer 与共享语义 IR；逐模块拆分 |
| `eggfuse/compiler/*` | compiler 编排；旧 Python 转发入口已经删除 |
| `tools/cell_spine/*` | 将稳定编译 pass 与独立研究原型分开，不能整目录照搬 |
| `eggfuse/compiler/targets/metal/compiler.py`、各 `lower` 模块 | compiler 的 Metal target，不能算 engine 的“识别能力” |
| `engine/metal/worker.swift`、运行时 `.metal` | engine；只依赖 Device/Recipe 契约和底层运行原语 |
| `frontend.rs`、`compare_runs.py`、snapshot/extraction 检查 | CLI/frontend 或 validation；独立于计时循环 |
| `test_*`、fixture、profile/bench 脚本、`*-results.json` | tests、benchmarks、实验报告 |
| 从 `experiments/polyspine_ridge` 导入的配方编译逻辑 | 提取稳定部分至 compiler，模型训练留在 experiments |

依赖方向应为：`analyzer → IR`、`compiler → IR`、`engine → Device IR`。
Validation 可以同时驱动它们；engine 不反向导入 analyzer 或研究实验来重新判断规则语义。

## 实施顺序与验收

1. **本轮：统一项目名称、公布真实组件/能力地图。** 保留原生 `egg_layout` 包和 CLI；已迁移的 Python/Metal 路径不再保留兼容入口。
   不混入 Git remote 发布或整仓路径重写。
2. **统一 Recipe IR 与 verifier。** 适配 generic 和 terms 两类现有产物；符号改名后仍可识别
   同一结构，且保持所有 guard、参数关联、effect 与控制。先用原解释器作语义基线。
3. **补 context 的生成/发布契约。** 支持结构增长、constructor set、union、间接 merge 的
   证据及外部读取边界。覆盖迟到输入、多见证、alias、重入和有限 Run；未知情况可精确回退。
4. **将 always-run 按真实数据依赖形成可执行区域。** 不要求整个 222-rule ruleset 一次通过
   一个狭窄证书。区域内固定/可变端口和 effect 必须共享同一个 Run 输入视图；不能提早提交
   一组规则，让另一组读取新状态。使用按规则/阶段归因的成本选择区域。
5. **贯通事件发布、延续与失效。** Marker 带实例关联，不靠再次查遍 LHS 确认实例存在；
   公共端口共享；更新/union/subsume 触发必要的局部更新与原路径回退。
6. **再做布局与融合。** 按相同接线批量执行，固定算术与预测分开，保留精确身份；以真正
   减少重复查询/推导为目标，不以更多矩阵、更多 Cell 或更多 macro 调用为目标。
7. **沿上述边界逐目录迁移。** 每次只迁一层，迁移脚本/skill 后删除旧路径，检查原生结果、
   产物加载与 benchmark 配置。路径变动与语义变动分开提交；不为了搬文件重复跑全部历史。

每条规则应有 `可表达 → 可验证 → 可优化 → 本次实际执行` 的状态及拒绝原因。每个原阶段
报告获认证区域、残余工作、上下文建立/刷新、必要 effect、发布/rebuild 和总耗时。三组
融合实验仅得到约 1.1%、0%、0.25% 的变化，默认未启用；不能再用它们替代覆盖建设。

## 证据入口

- [统一编译流程](../../eggfuse/compiler/README.md)
- [完整 Pass 3 与原生差异](../../experiments/unified_compiler/PASS3.md)
- [联合执行消融及覆盖口径](../../experiments/joint_execution_fusion/README.md)
- [Rust recipe 导出](../../src/cell_recipe.rs)
- [通用 GPU Cell 执行](../../eggfuse/compiler/targets/metal/generic_cell.py)
- [特定多阶段配方 exporter](../../eggfuse/compiler/recipes/executable.py)
- [联合快路径契约](../../eggfuse/compiler/targets/metal/joint_polyspine.py)
- [固定载体矩阵准入](../../eggfuse/compiler/targets/metal/matrix_sat.py)

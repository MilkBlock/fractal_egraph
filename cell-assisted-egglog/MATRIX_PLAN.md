# Cell 矩阵执行主线：重新规划

2026-10-18。本文是实施规划，不表示下述整合已经完成。

## 目标与纠偏

主线是：**marker 改变信息暴露深度，分级矩阵匹配保留未决 binding，
Cell specialization 提供可复用的目标构造，analyze 提供 FractalComb 的证据。**
GPU 需要做跨 Cell、root/non-root 触点的条件计算与转移，不只是判定一个 Cell
内部的 RipenInput 是否相等，也不是逐表搬运 Free Join。

`resident.metal` 的哈希查找版保留作真实事件接入、精确语义对照及过渡后端。
不再以增加 Fact/Watch/Partial 等结构、索引和专用 pass 作为最终架构的主线。
其事件桥接、i64 处理、原生 RHS 提交与回归测试仍然复用。

统一表示不是把结构体数组改名为矩阵。marker、端口接线、分层条件、有效性、
参数更新和目标坐标必须属于**同一份有类型的矩阵计算图**；布局和 stride 由它生成。
多个数值类型/矩阵块以及多个底层 kernel 可以存在，但不各自定义新的 Cell 语义。

## 已有实现：迁移，不重新发明

以下边界来自本次源码和实验说明检查；本次没有重新运行旧实验。

| 组件 | 复用的内容 | 必须保留的边界 |
|---|---|---|
| `../experiments/cell_spine/marker_context/` | 将深层信息暴露为浅层坐标；共享 marker；按依赖增量更新 | CPU 原型、有限 Hadamard 域；不能直接沿用到重复输入和任意 union |
| `../experiments/cell_spine/layered_context/` | complete/rejected/continue；同形条件批处理；跨层证据复用 | 原型控制流程在 Python；候选外层连接由实验提供 |
| `../experiments/cell_spine/moment_compiler/` | signed-bit 身份编码、二阶 pair 坐标、公共计算 DAG、Metal/MPS | 交换不变 pair 编码不是任意 AC/大多重集编码；稠密输出仍可平方增长 |
| `../experiments/cell_spine/transition_program/compiler.py` | 完整父上下文保留；目标槽位 gather；下一阶段消费目标坐标 | 有限已知转移域；不能仅以 RHS 代替父上下文 |
| `../experiments/cell_spine/transition_program/runtime.py` | 真实外部 fact 驱动、union 失效、alias specialization 与恢复 | 仍有 CPU 路由、Bake 和完整闭包 key 的成本 |
| `../experiments/cell_spine/transition_program/specializations.py` | 新 binding 复用已证明的参数接线；缓存命中跳过匹配/Bake | 构造器域的证明不能直接迁移给任意 guard、function 或 merge |
| `../src/native_cell_families.rs`、`../src/native_cell_ports.rs` | analyze 的 producer/effect、重复关系与端口推进证据 | 有限观察不是无限展开证明；不以源 AST 扫描替代实际历史 |
| `src/resident.rs`、`../egglog/src/cell_events.rs` | 提交事件、union/rebuild 通知、GPU 常驻和原生 effect 接口 | 现有实例有效性维护不等于 Ti → Tj 的闭包状态机 |

旧的 `parameterized_cells` 表达式编译器可作为算术/接线 lowering 的参考与测试；
它的 AST 递归发现不是 FractalComb 的权威来源。

## 一条执行链

```text
analyze 的实际历史、Cell 状态、接线和重复证据
    ↓
公共 matching spine + marker 计划 + 分级条件 + specialization/转移契约
    ↓
统一 diff form：端口坐标、marker、数值参数、有效性与当前条件证据
    ↓
真实外部连接 / 原生提交事件驱动的活跃 binding 通道
    ↓
浅层矩阵批次
    ├─ 完整 binding 已证实 → 进入目标构造
    ├─ 所有接线分支均失败 → 排除这个 binding 通道
    └─ 仍有可满足分支      → 仅暴露缺失的下一层条件
                                   ↓ 继续共享矩阵计算
    ↓
匹配成功，但目标表示的 alias/参数前提不成立？
    ├─ 是 → 保留成功 binding，等待 specialization，恢复原任务
    └─ 否 → 旧 diff form + 新 binding → 新 diff form
    ↓
目标 Cell 的 marker/坐标成为后续转移的输入
    ↓
按明确提交边界执行原生 effect，处理全局 union/rebuild，反馈失效与新证据
```

**continue 是匹配信息不足；needs-specialization 是匹配成立但目标表示不可用。**
不得把两者都压成 false。一个 binding 已完成，不代表同一触点对的其他 binding
可以丢弃。条件 OR 的不同分支保留各自完整接线，不能将两个分支的半个证据拼起来。

深度表示尚需读取的上下文深度，不是 egglog round，也不是固定要求读取 d 层。
一个预计算 marker 可以把深层问题提到第 0 层。其他任务已经计算出的证据，也可以
让深层任务立即完成；不必为维持“多阶段”而机械执行每一级。

## 统一程序的最小内容

只保留三类编译信息，不引入另一套 Cell/Spine 命名体系：

1. **坐标定义**：类型、形状、端口角色、有向 slot 接线、marker 依赖、编码前提。
   对称坐标负责比较；RHS 仍需要的有向接线不能被对称化抹掉。
   数值参数与身份编码分别有精度契约，不能将 native value ID 当成数值。
2. **条件计算图**：投影、lift/moments、矩阵乘法、距离/比较、AND/OR 与未决状态。
   共用同一相对接线的条件只计算一次；共享到哪些完整 binding 通道是显式的。
   未证实为充分条件的浅层 marker 只能筛选/提供部分证据。
3. **转移计算图**：目标 Cell 身份、目标 slot 映射、参数更新、目标 marker 更新、
   effect 和 specialization 前提。模板和输入坐标隐式保留完整上下文；所有后续
   仍可能查询的中间事实/union 不得因只保存最终 RHS 或 cost 而消失。

FractalComb、match 与 graft 共享这一套计算图；反向依赖由计算图生成。
边界事件仍可以用紧凑协议接入，但不能成为另一套重复维护全部 Cell 含义的系统。

矩阵批次按接口形状、角色、编码维度和相对接线组织，不按真实 binding 值缓存
每个组合，也不枚举所有 Cell 对。输入外层连接必须实际存在，例如 factoring 的
外层 Add，不能因为两个 Mul 的因子相同就凭空接受这个 Add。

## 实施顺序与交付条件

### 1. 先统一旧实验的程序表示

- 在 `src/program.rs` 定义上述类型/形状、共享计算图和证据范围。
- 在 `src/compile.rs` 接 analyze、现有 spine/marker/depth/transition 描述。
- 复用 moment 的编码与 Metal/MPS 算子，不再写一份新的身份匹配算法。
- 生成 Rust/Metal/Swift 所需布局，结束三个语言分别维护语义相关 stride 的方式。
- 区分“观察到的族”“完整具体状态相同”“可以跨 binding 迁移的转移”。
  共享完整具体状态不应仅因为出现 primitive 而被拒绝；参数化迁移则保留条件、
  算术与 merge/schedule 语义，不能由一次 ripen 成功自动推出。

**验收**：同一份程序由 CPU 参考与 GPU 解释；已有 marker/moment/两阶段转移
样例的完整 binding 和展开状态相同。原实验作为回归基线，不再各自扩展一套框架。

### 2. 接回 marker + 分级矩阵匹配

- marker 由已验证 Cell 接口生成，不用 native Join 重新证明已保证的内部结构。
- 合并重复条件计算，保留完整候选 binding 的 OR 分支和位置。
- GPU 维护 complete/rejected/continue 的掩码，只推进未决通道。
- marker 和特征按变化坐标更新；未受影响的块与已有证据直接复用。
- 不加隐藏的 Top-K、代表选择或深度预算；深度来自接口/有效转移定义。

**验收**：root 和 non-root 均与一次性精确匹配一致。覆盖“浅层完成”“浅层否决”
“确需深入”“同叶集合不同括号”“重复输入”“同触点多 binding”。同时报告
矩阵实际位置、padding、多算位置、marker 构造、更新、compaction 和调度成本。
原来 root-only 有收益而全触点无额外收益的负结果必须继续存在。

### 3. 在同一程序内完成目标构造和 specialization

- 复用 target gather 和两阶段 GPU continuation，保留完整父上下文及新 effect。
- alias、数值条件、接线变化引起的 specialization 都绑定到明确的前提与环境。
- 已匹配但需要 specialization 的工作保留，修复后从该位置继续，不重做已完成匹配。
- specialization 证明命中时直接更新目标坐标，不强行再跑 GEMM；计入 guard/更新成本。
- 专化只能折叠被证明一致的域；不能将数值参数擦掉冒充 opaque 输入对称性。

**验收**：把旧的 union 缺失反例接入统一程序：旧接口的编码更新不冒充完整性，
专化后恢复其遗漏 binding。复用旧实验的“目标只存 RHS 会丢事实”反例。
固定、已验证的两阶段局部转移不得在阶段间回 CPU 做匹配或搬回完整结果矩阵。

### 4. 接入真实原生事件与提交边界

- 保留现有 committed row / union/rebuild 事件和 compact producer bindings。
- 用这些事件登记 Cell、维护真实边界连接、定位受影响坐标；不要求每轮整图导出。
- 优先使用已知状态和 graft 的证明更新坐标；缺证据的部分保留为未决，不伪装成失败。
- 原生 RHS/custom merge/checked arithmetic/panic 是语义基准；会改变全局代表元
  的提交设置明确的同步边界。GPU 局部虚拟结果不能提前冒充全局已提交事实。
- 原生节点查询和库外区域的处理必须显式计费、显式报告覆盖；旧哈希 matcher 仅作
  对照/过渡，不让一个未注明的全量 Join 掩盖矩阵路径的缺失。

**验收**：late fact、跨 Cell union、subsume、function set、别名专化、失效恢复
均可复现。34 项现有运行时回归继续通过。跨层实际路由的完整性单独验证。

### 5. 将 analyze 的 FractalComb 降为程序压缩

- 只消费 analyze 中有版本的 producer/consumer、相对接线、条件与 effect 证据。
- 首先共享重复的转移子程序，再将有稳定接线的已验证重复段表示为参数化重复。
- 仅在坐标更新闭合且条件/数值语义允许时使用矩阵幂或组合算子；不是看到重复
  rule 名就假定存在固定 M。参数衰减、外部 fact 义务和分支位置全部保留。
- 多 apply point 用共享子程序与不同接线表示；不同执行顺序不自动交换。
- 有限观察允许压缩已观察实例；任意 n 的外推需要另行证明，unfinished 不标饱和。

**验收**：从 analyze 数据导入的线性、交替、分叉案例，逐次执行与压缩程序的完整
binding/effect/必要中间状态一致。未知域、额外外部义务和不满足数值条件的例子
不能通过；不能只比参数最终值、节点数量或最终抽取表达式。

### 6. 最后在真实 eggcc 快照验证收益

继续使用 raytrace Pass-3 的 packet 349，保留 14 条规则和原 schedule。
三个真实检查点的 14/14 query conformance 是已有正确性门槛，不是 Cell 覆盖或加速。

固定同一输入、边界连接、有效程序域与最终状态，分离以下消融：

- 原生；旧常驻哈希对照；统一程序的一次性矩阵匹配。
- 相同程序 + 分级推进。
- 相同推进 + marker。
- 再加入 specialization 复用和经验证的 FractalComb 压缩。

报告：完整 binding、effect/可见性、最终等价图与适用时的 extraction 质量；
有证据的 Cell/转移覆盖与 pending；准备/维护/传输/GPU/原生提交时间；
实际设备内存与矩阵总工作。不能以库大小、逻辑 MAC 或存储压缩替代匹配复用收益。

## 本轮范围

仅重新规划并确定已有代码的迁移职责；没有改写或宣称完成统一矩阵运行时。
后续第一项实施任务是第 1 步及其回归适配，随后立即接第 2 步；不先扩写
旧哈希 matcher，也不另起一个只证明输入 ID 相等的孤立 demo。

## 实施记录：统一矩阵程序首段

已实现 `src/program.rs`、`src/compile.rs`、`matrix.metal`、`matrix.swift` 和
`--mode matrix`。旧 marker/分级接口与两阶段转移通过
`tools/cell_spine/matrix_program.py` 导入同一 IR；CPU/GPU/native 的核对见
[matrix-program/README.md](matrix-program/README.md)。

已经接通的部分：共享坐标和模板描述、GPU 三态推进、按需特征维护、实际 MPS、
目标 gather、目标 alias-pending、增量更新。MPS 紧凑尺寸仍由主机读取计数后提交。

没有将整个路线标为完成：analyze 的直接前端和认证拆分、自动 specialization 回流、
原生事件到矩阵路径的接入、FractalComb 压缩、真实 eggcc 完整加速均仍待实现。
下一步应接 analyze 证据与目标专化，不再增加一个平行的身份编码 demo。


## 实施记录：analyze 前端与已认证专化接续

`--mode matrix --analysis DIR` 已直接导入 analyze 的 closure-port-progressions：
真实 producer/read 版本、完整 LHS、共享输入接线和父闭包保留证明都经过校验。
当前范围是两读端口契约，未覆盖的 guard、内部结果等式、额外义务不会被擦除。
三段源 Cell 的实际样例导入了两个边界转移，CPU/MPS 与原生查询核对通过。

目标构造新增已认证 alias 专化表。同一 GPU 程序在成功匹配后检查完整参数分区，
投影到专化模板的新槽位，然后继续后继匹配；不以修改旧模板标签冒充闭包修复。
两个 tiny-math 专化的完整闭包与原生相同，并恢复两个后继 factoring binding。
未变化的下一轮不重新计算 GEMM 或特征，仍有控制和 guard/gather 成本。

边界仍然明确：首次专化由离线 native Bake 证明后重新编译，尚未热插入常驻程序；
analyze 导入只执行已观察连接，尚未接全局原生事件服务。数值专化、一般 union
后新增触点的自动发现，以及 FractalComb 子程序压缩都没有因此自动完成。
验证与复现命令见 matrix-program/README.md；旧哈希事件后端继续作为语义对照。

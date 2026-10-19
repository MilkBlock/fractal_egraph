# fractal_egraph

**从规则执行历史中学习局部闭包，把重复的推理编译成可复用的 Cell 和边界转移。**

这是基于 [egglog](egglog/) 的分析与编译研究原型，Rust 包和命令行仍叫 `egg_layout`。
项目希望复用一整段推理已经得到的知识，减少重复匹配、重复构造和中间状态转换。
当前可执行主线是 **RipSatCell → canonical spine → 生成 `.egg`**；压缩率和速度需要分别实测。

新增[完整 RipSatCell 转移执行对照](experiments/whole_cell_chain/README.md)：原生 Bake 自动生成
A → B → C 的完整闭包增量，运行时跳过原始 smooth 匹配，并逐阶段核对全量事实、等价关系与 extraction。
这是 CPU/native 提交后端，不是接口缓存或 GPU 加速比结论。

新增[原生事件驱动的整 Cell 接续](experiments/whole_cell_automatic/README.md)：多个实例由实际 coarse 提交自动选择完整转移；未知边界保留原生 fallback，逐轮核对完整图。

新增[GPU Cell locality 分级与复用对照](cell-assisted-egglog/cell-locality-bench/README.md)：按部分 binding 定位已知 Cell，携带 context 与唯一前缀索引分流；分别记录实际匹配复用、完整匹配产出和分级开销。

## 方法总览

```text
规则 + 训练 facts
    ↓ 原生执行，记录实际 binding、读取、写入和 union
局部应用历史 + 外部依赖
    ↓ RipenInput 抽象、按阶段 replay、局部 ripen
已完成的局部闭包
    ↓ 在规则环境和接口约束下判等
Cell egraph：共享的闭包状态 + 多条生成路径
    ↓ Bake：选择 spine、marker、接口和转移实现
生成的 .egg：原始输入 → Cell 化 → 边界转移 → 显式展开结果
```

### 1. 从真实依赖发现局部性

一次 rule apply 带有具体 binding：它读了哪些 fact、由谁提供、在哪个位置应用，以及实际提交了什么 effect。
历史给我们一个有根据的局部组合，不必先枚举任意子图或任意规则排列。

**Coarse / Smooth 是相对于当前组合的分类：**

- **Smooth rule application**：所需上下文已经由本组合提供。
- **Coarse rule application**：还需要外部 fact、等价关系或其他条件。

同一条 rule 在不同组合中可以属于不同类别。单个逻辑 match 也不等于一次有效写入。
当前默认发现结构是已提交应用的状态 DAG；另有单次应用、producer–consumer 的历史窗口，
允许从历史中段提取 Cell。旧的 prefix 分块、CSCS／CCSS layer 策略不是默认发现主线。

### 2. Ripen：为一个局部入口求闭包

提取局部历史时，外部 e-class 输入用有类型的 `RipenInput` 表示；保留参数位置、别名、字面量、
等价条件及外部 fact 的注入时序。它不是可以忽略约束的万能通配符。

**Replay** 按已有 binding 检查条件、执行已记录动作，验证抽取后没有丢失必要上下文；
**ripen** 再运行允许的原规则，尝试达到局部不动点。这是两个步骤。

在受支持的有限正向片段中，可以把合法入口记作 K，把固定规则环境记作 R：

```text
D = Sat_R(K)
```

完成的 D 称为 **RipSatCell**，即一个 saturated rule composition 的局部闭包。
这里的饱和相对于该规则环境和抽象边界；接入新的外部事实或 union，仍可能启用新的转移。
超时、超量或无法导出的观察状态不会被当作已认证闭包。

### 3. Cell egraph：同一个状态，可以有不同历史

不同区域、不同执行顺序、不同 coarse 入口，可能得到相同闭包：

```text
入口 K₁ ── 一段历史 ──┐
                      ├── 同一个 Cell D
入口 K₂ ── 另一段历史 ─┘
```

闭包判等检查规则环境、类型、字面量、接口、等价关系和可见性。
Rust 维护历史与 binding，原生 Cell egraph 登记构造并利用已认证判等归一化后继。
共享的是闭包状态；各条路径的参数对应和外部义务仍单独保存。
一次 **graft（嫁接）** 将新的上下文接到 Cell 边界，形成后继 Cell；转移保留 rule、relative binding 和新增 effect。

图中还区分两类连接：**观测历史**说明实际先后经过什么，**已验证嫁接**说明在明确条件下可复用哪个前驱。
曾共享一个节点，不足以证明前者是后者的必要依赖。
同一 Cell e-class 的两种端口排列，也不能在没有 binding 对应的情况下互换。
见 [左右 graft 与 union 回归](experiments/cell_spine/graft_diamond/README.md)。

### 4. Bake：把闭包知识变成紧凑的运行时表示

一个 Cell 可以有多种等价生成骨架。我们选择一个 **canonical spine**：保留多个小节点，
在选定位置放置携带必要字段的 **Summary / marker**，表示这个 Cell 实例处于可用状态。
实例绑定可以不同，但共享同一份闭包和转移描述。

在 canonical 存储模式中：

| 表示 | 用途 |
|---|---|
| 原始 `Op` | 原程序的语义节点，供基准和最终结果使用 |
| `Op!` | 新输入进入压缩运行时的节点 |
| `Op!!` | 选中的 canonical spine 节点 |
| `Summary-N!` | 活跃 Cell marker，字段和附着位置由求解器选择 |

生成的规则匹配活跃 marker、必要的边界 fact 和残余条件，构造新 spine，发布新 marker。
被大 Cell 接管的小 marker 可以 `subsume`；已有子树可以继续共享，大 Cell 继承已证明的后续转移。
`subsume` 停止匹配，**不等于物理回收表行**。求解器也可选择借用已有原始行，避免一律复制 spine。

例如两个小 Cell 加上真实的连接 fact，可以直接进入一个更大的已知 Cell；不必逐个展开其内部推理。
但两个 Cell 仅仅同时存在，并不授权把它们合并。

**Spine 和 marker 是联合选择的。** 求解器同时考虑转移两端的布局、可共享子树、保留字段、
残余 Join、写入和共享接口维护成本；不能先贪心选 spine，再独立决定 marker 放哪里。
默认使用 SCIP，也可显式使用 Gurobi。最优性只针对已定义的候选表示与成本模型。
`--storage-cost instances` 进一步纳入当前输入实际保留的表行槽位，仍不是完整 RSS 模型。

压缩推理运行生成的专用 ruleset，结束后由 `Export!` 显式还原原始节点。
原规则可以保留在输出源码中，但不会作为隐藏闭包上的自动 fallback 执行。
详见 [编译器、求解模型与运行时约定](tools/cell_spine/README.md)。

### 5. 扩大 Bake 的适应性

仅记住训练时走过的路径，会漏掉到达同一个 Cell 的其他入口。目前有两种补充方式：

- **跨输入知识库**：在相同规则／声明／schedule 环境下保存共享闭包、入口和 binding，供以后 Bake 引入并重新验证。
  合并族学习压缩这些转移描述；归纳出的通用模式暂不直接授权未知转移。
- **LHS 覆盖与替代入口**：索引哪些规则能完整或部分匹配已知 Cell，保留共享变量和缺失条件。
  再检查该规则的 RHS 能否补出旧入口的 fact，推导新的入口。

后者的核心保证是：已知 Sat(K)=D，如果新入口 E 全部位于 D 内，且一次合法应用能够从 E 补齐 K，
那么在该单调规则环境下 Sat(E)=D。完整／部分覆盖本身不等于这个结论。

例如只从 P(x) 训练得到含 P、Q 的 Cell，仍可利用 Q(x)→P(x) 发现 Q 入口。
它没有新的训练事件，也没有新增 Cell 家族，却扩大了固定库能接受的输入。
当前 `--lhs-coverage entries` 实现单步入口替换；`report` 只出表，默认 `off`。
见 [跨样例库](experiments/cell_spine/knowledge/README.md) 与 [LHS 入口对照](experiments/cell_spine/lhs_coverage/README.md)。

### 6. Fractal rule 是进一步的研究方向

如果 Cell 的边界转移、relative binding 和 effect 出现稳定的递归规律，就可能用参数化合并族表示一批组合。
耗散型还受到额外节点或递减条件约束；持续自启动的形态则可能不收敛。

当前系统提供 weak-dominate 查询、非平凡再入观察、结构分组及有限转移描述学习。
**一次再入、超时增长或重复 rule 名称，都不是无限递归或归纳等价的证明。**
任意规模 FractalComb、自动推导 FlashAttention、GPU 矩阵化推理属于研究方向，尚非已交付能力。

## 已验证到什么程度

生成可执行库前，对训练入口、引入的历史入口和启用的新入口运行原生对照，比较完整 constructor 图与等价划分。
只比较 e-node 数量不足以证明正确。无法验证时失败，不把完整快照归档伪装成 Cell 压缩。

| 已有实验证据 | 结论的范围 |
|---|---|
| [未见入口对照](experiments/cell_spine/lhs_coverage/README.md)：9/18 → 18/18 | 固定训练库，测试输入不重新 Bake；包含反向参数、共享变量、union 与多 fact 入口 |
| 同一实验的 math 子集：展开式还原 9 → 13 个节点 | 完整结果与原生一致，Cell 家族仍为 3 个；没有证明任意新规模都可复用 |
| [Spine 存储表示](experiments/cell_spine/representation/README.md) | 某些场景减少实际存储行；不代表所有程序都省内存或更快 |
| [共享接口](experiments/cell_spine/shared_interfaces/README.md) | 减少重复解码与 effect；新增维护规则也有成本 |

原生分析的支持面比 Cell 编译更广。当前自动 spine 编译要求自包含、单规则集饱和 schedule、
受支持的 constructor／等价操作及完成的闭包证书；函数更新、primitive 条件、隐藏行等仍有分层支持边界。
这是**已 Bake 转移的白名单执行**，不保证覆盖未来输入中每个隐藏 fact 的所有新匹配。
匹配 binding、精确判等和布局求解仍可能很昂贵，有限局部图也不意味着组合数很小。

## 快速开始

在仓库根目录运行。需要 Rust 与 Python；首次构建需取得 Cargo 依赖。
Bake 的 Python 依赖包含无需商业许可证的 SCIP。

```sh
python3 -m venv out/cell-spine-venv
out/cell-spine-venv/bin/pip install -r tools/cell_spine/requirements.txt
cargo build --release --bin egg_layout \
  --example lhs_ast --example spine_validate \
  --example spine_stages --example native_math_baseline

# 小型 math 训练输入；输出目录应为空
out/cell-spine-venv/bin/python -m tools.cell_spine.compiler \
  --source experiments/cell_spine/math_tiny/source.egg \
  --output out/my-cell-bake --skip-build --lhs-coverage entries
```

`library.egg` 是生成的可复用规则库；`runtime.egg` 包含当前输入及压缩执行；
`restore.egg` 再显式还原结果。`report.json` 记录表示选择和成本，`validation.json` 记录原生对照，
`lhs-coverage.json` 列出完整／部分匹配、binding 和新入口依据。
可添加 `--knowledge-library .egg-fractal/knowledge.json`，在兼容环境下积累跨输入知识。

只做分析、查看 Cell 图时：

```sh
cargo run --release --offline -- analyze \
  --source experiments/cell_spine/math_tiny/source.egg --output out/my-cell-analysis
python3 tools/egglog_debugger/server.py --port 8080
```

网页依赖本地 Node、Graphviz、Typst、Eggplant 渲染插件及相邻的 egglog-demo 资源；
详见 [本地调试桥](tools/egglog_debugger/README.md)。启动后可打开
[独立 Cell 查看页](http://127.0.0.1:8080/cell-viewer.html)，选择分析目录和轮次，点击 Cell 或嫁接边查看内部图、规则与 binding。

`analyze` 默认启用历史窗口；Bake 当前默认保留 rooted capture 作为对照。
设置 `EGG_LAYOUT_CELL_WINDOWS=1` 可让 Bake 也使用中段窗口。
分析遵循源 schedule，`--rounds N` 只覆盖支持的简单 run 次数。
Ripen 默认观察上限为每个 Cell **50 ms / 200K facts**，达到后保留 unfinished 状态；这不是整个进程的内存或硬实时上限。
`--save-history` 保存离线分析所需的历史，`--replay-history FILE` 可重新分析。
`EGG_LAYOUT_GRAPHS=0` 关闭图表输出；无图模式不生成逐轮展示快照。

## 代码入口与复现

| 部分 | 入口 |
|---|---|
| 原生历史及局部组合 | [native_analyze.rs](src/native_analyze.rs)、[native_cs.rs](src/native_cs.rs) |
| 边界抽象、回放与饱和 | [native_ripen_entry.rs](src/native_ripen_entry.rs)、[native_ripen.rs](src/native_ripen.rs) |
| 闭包判等、Cell 图与嫁接证据 | [saturated_rule_composition.rs](src/saturated_rule_composition.rs)、[native_cell_egraph.rs](src/native_cell_egraph.rs)、[cell_graft_interfaces.rs](src/cell_graft_interfaces.rs) |
| Bake、布局求解和 marker lowering | [tools/cell_spine](tools/cell_spine/README.md) |
| 新入口及跨样例知识 | [lhs_coverage.py](tools/cell_spine/lhs_coverage.py)、[knowledge.py](tools/cell_spine/knowledge.py) |
| 原生共享存储等独立研究路径 | [Reuse Graph](docs/reuse-graph.md) |

```sh
# 固定训练库，对比未见输入；每次使用新目录
out/cell-spine-venv/bin/python experiments/cell_spine/lhs_coverage/probe.py \
  --output out/my-entry-transfer
out/cell-spine-venv/bin/python -m unittest tools.cell_spine.test_lhs_coverage
cargo test --release --offline --test graft_diamond --test cell_windows
```

[GitHub CI](.github/workflows/cell-benchmark.yml) 包含契约回归、表行规模和分阶段时间对照。
`egglog-baseline` 标签保留 pristine upstream 导入；本地内核修改、实验代码和结果分别记录。
默认使用本地调试网页，不自动发布 gh-pages。

---
name: cell-assisted-egglog-bench
description: 运行和解读 cell-assisted-egglog 的 CPU/GPU、稳定选择、整图一致性、Tiger extraction、增量查询 audit、差异定位和 sat-skip microbench。用于 egg_layout 的辅助运行时验证与性能对比；eggcc 原生三 pass 的 Saturate/Run 耗时统计使用 eggcc-pass-benchmark。
---

# Cell-assisted egglog 验证与计时

使用本 skill 的 `scripts/bench.py`，不要重新拼 Rust、Swift、Tiger 命令或另写统计器。脚本默认使用本 skill 所属的版本化 checkout，`--repo` 可指定其它 checkout；会显示实际路径。原始快照和生产 Tiger 默认来自共享的 `Repos/egg_related`，不假设 worktree 旁边有 check_statewalk。

`$SKILL` 指本目录。用 `/opt/homebrew/bin/python3`，或其它 Python 3.9+。

## 自动 Cell 账表

成功的 `run` / `audit` 自动写出 `cell-ledger.md`、`.csv`、`.json`，包含所有 Analyze Cell（含无计划/拒绝项）、共享片段、源码程序和证明组。普通 `run --all-rules --analysis DIR` 自动复用或补齐独立 plan-only/复用诊断，不污染原计时、不重跑未变 CPU。`--no-cell-metrics` 只禁用额外采集；缺测保持 null。已有目录用 `cell-ledger --input DIR [--baseline PLAN_DIR]` 一次生成，禁止再为同类指标临时写脚本。归因与字段详见 [Cell 账表](references/cell-ledger.md)。

## 默认复用历史

按用户约定，假设同一台机器的性能稳定。**先读已有结果，再决定是否执行。** 不为刷新日期、增加平均次数或陪跑未改的对照而重测；也不要重复解释已经固定的底层命令。

- 先用 `history --joint-sat` 找同输入、同选择策略、同实验模式的已通过记录，再 `report 目录`。去掉 `--joint-sat` 查普通 GPU 对照。按最近的适用版本选择基准，不挑最快一次。
- 未改变的 CPU、GPU-off、输入准备、Tiger 结果可作为历史对照。保留其目录、版本、配置和时间，不把历史时间写成这次新测。候选实现改变时，旧结果不能证明新实现正确。
- `run` 在增量构建后，若输入、实际 Rust/worker 二进制、策略、joint 开关、记录的运行配置和机器一致，会直接返回已验证结果；不重新推理、不创建新的 output 目录。需要 Tiger 时，还核对提取工具与参照输出的指纹。`--refresh` 才强制重测。
- 完整 `run --all-rules` 会保存原生、同提交协议 CPU、GPU-off 的完整事实及冻结输入锚点。后续用 `run --all-rules --joint-sat --reuse-controls 旧目录` 只运行新 GPU 候选，重新核对完整事实、值、等价关系和可见性；旧时间明确标为历史。要求输入、调度、分区和选择策略相同，且旧 CPU/GPU 图检查通过。旧目录缺少 `graphs/*.control.json` 时不能使用此模式。
- 缓存校验使用完整结构比较，不以哈希代替判等。native/CPU 的 Tiger 结果只在提取器和输入/输出指纹均一致时复用；新 GPU 的提取与检查仍执行。只读询问仍用 `report`，不调用 `run`。
- 基线二进制、输入/调度、线程/worker、选择策略或验证口径改变时，先判断哪部分对照受影响；只有受影响的结果才需要刷新。公共内核变化可能同时影响 CPU 和 GPU。

## Cell / Join 改动的固定评价指标

**Cell 选择使用反事实 credit；GPU 工作迁移只作定位。** 空表短路、普通函数键取值、固定计划和执行后端迁移不能自动给 Cell 领取 credit。对照应保留这些公共能力，只撤去目标 Cell 结论/证明。Switch 空回答不得领取 Cell credit；Get 的新端口读取仍记成本。此前全服务开/关的起点/行访问差值只叫“观测差额”，不用于 Cell 排名或训练。详见 [credit 协议](references/credit.md)。

**Join 深度分布是评价一套 Cell 与 Join 流程的必报指标。** 先复用已有同口径诊断；只有候选实现或数据流改变才补测，不为生成报告重跑未变对照。至少给出：

1. 外层起点 `eligible_seeds`，和逐层进入数、活候选行访问数；分别回答省了多少外层启动、多少内层工作。
2. `Pr(max_depth >= d)` 的前后曲线、最大到达深度直方图、各层证否占比和各层节省的行访问数。概率必须来自 `deepest_seeds`，不能用循环进入数冒充。
3. 按原始 rule 拆分，并给出固定规则权重的曲线；说明新增、移走的查询。规则构成改变与同一候选更容易被证否须分开。固定规则权重仍不是固定候选身份。
4. Cell 提供约束时，另报完整且版本有效的约束命中、未知/失效回退、约束导致的证否，以及其维护时间、传输和存储字节。不能用正例命中率代表完整 binding 范围。
5. 另用无插桩运行报告端到端推理时间和整图检查；诊断 atomic 的时间不用于宣称加速。新约束消融须固定 Cell 查询分流、Join 顺序及其他证否开关，才可归因。

优先用下表的 `depth` / `interaction` 读取已存数据。若历史缺字段，明确写“未测”，不要补造概率；当前统计对象为 compact GPU Join，不能冒称原生 CPU Free Join。

## 常用入口

| 需求 | `bench.py` 子命令 |
|---|---|
| 通用 GPU 空表短路对照（不使用 Cell） | `empty-tables --before 关闭目录 --after 开启目录 [--timing-before DIR --timing-after DIR] [--output NEW.json]` |
| 联合闭包证明组的净 credit | `cell-credit --baseline 仅关闭证明 --candidate 开启证明 [--output NEW.json]` |
| GPU 工作迁移与局部成本账（非 credit） | `cell-value --input 诊断目录 --before 历史Cell关闭 --after 历史Cell开启 [--output NEW.json] [--csv NEW.csv]` |
| 每个 Cell 程序的真实缓存率与案例 | `cell-cases --input 诊断目录 --before 历史Cell关闭 --after 历史Cell开启 [--output NEW.json] [--csv NEW.csv]` |
| 看已有结果，不重新跑 | `report [分析目录或 report.json]` |
| 查找可复用的完整 Pass-3 历史 | `history --joint-sat` |
| 固定 Cell 分流，比较完整 binding 约束关闭/开启 | `depth-pair --before DIR --after DIR [--output NEW.json] [--csv NEW.csv]` |
| 只读查看 Join 深度分布及导出 CSV | `depth --s0r0 DIR --s0r1 DIR --s1r0 DIR --s1r1 DIR [--output NEW.json] [--csv NEW.csv]` |
| 只读分析 sat-skip/refutation 的交叉计数与规则构成 | `interaction --s0r0 DIR --s0r1 DIR --s1r0 DIR --s1r1 DIR [--sat-only DIR]` |
| 核验输入、工具与缓存路径 | `check` |
| 真实 raytrace Pass 3 稳定选择计时＋整图检查＋Tiger | `run --output out/新目录` |
| 所有原始 Pass-3 规则交给 GPU，同时保留 Cell sat-skip | `run --all-rules --joint-sat --output out/新目录` |
| 全规则 GPU 剩余查询逐批核对 | `audit --all-rules --joint-sat --output out/新目录` |
| 仅核对 Cell 提供的完整绑定 | `audit --all-rules --joint-sat --cell-only --output out/新目录` |
| 只运行全规则 GPU 新候选，复用完整对照 | `run --all-rules --joint-sat --reuse-controls 旧目录 --output out/新目录` |
| 完整 Pass 3 的 joint sat-skip 对照 | `run --joint-sat --output out/新目录` |
| 只跑推理、推迟 Tiger | `run --output out/新目录 --no-extract` |
| 补做已有图的真实提取检查 | `extract 分析目录` |
| 重跑未改 `merge new` 的对照 | `run --policy original --output out/新目录` |
| 原策略 GPU 增量查询 oracle | `audit --output out/新目录` |
| 原策略 CPU 自对照 | `cpu-pair --output out/新目录` |
| 原策略首次差异定位 | `diff --output out/新目录` |
| Math 的认证饱和跳过实验 | `sat-skip --output out/新目录` |
| 参数化 terms 更新、union 与完整图一致性 | `parameterized --output out/新目录` |
| 已有多候选 Cell 的连续更新与端口复用消融 | `transition --output out/新目录` |
| 比较源码计划、文件加载和 joint 路径耗时 | `transfer-bench --output out/新目录` |
| 三个 terms ruleset 的联合 sat-skip | `joint-sat --output out/新目录` |
| 相关正确性回归 | `test` |

```sh
/opt/homebrew/bin/python3 "$SKILL/scripts/bench.py" check
/opt/homebrew/bin/python3 "$SKILL/scripts/bench.py" run --output out/raytrace-stable-new
```

执行类入口默认离线增量构建所需 Rust 示例；Swift worker 以源码/二进制指纹复用。构建不计入报告中的推理时间。仅明确复用已核验二进制时使用 `--no-build`；它不意味着缓存一定新鲜。全局 `--dry-run` 展示实际命令，不构建、不测量、不写输出。

默认是完整**已保存 Pass-3 输入和主调度的回放**，不是重新执行 eggcc 三个 pass，也不是 pristine upstream。允许 `--snapshot` 指定相同格式的其它完整快照；默认辅助分区为三个 terms ruleset；`--all-rules` 编译主调度中的所有原始 ruleset（含 combined 成员），遇到不支持的规则直接报错。稳定选择辅助规则 `cell-stable-keys` 仍在 CPU；RHS、merge、rebuild 仍由原生执行。不能据此宣称支持任意 `.egg`。缺快照就报告缺失，不换小 If/Math 冒充 raytrace。

## 验证和计时的界限

- 单次运行每条路径，不自动取平均。不要同时编译、audit、做图或运行其它 benchmark。
- `run` 的稳定策略在 CPU/GPU 共用同一原生 merge；主计时包含结构键维护，输入锚点准备和 worker setup 单列。脚本在整图失败后仍可做 Tiger 检查，但保留失败退出码。
- `run --joint-sat` 顺序跑原生融合、CPU 同提交协议、GPU 关闭 joint、GPU 开启 joint 四条完整主调度。只有 GPU 开启 joint 装载联合闭包 observer。报告 CPU/GPU 比值及 GPU 关闭÷开启的单独增益；后者必须通过 `joint_verification` 整图门槛，并展示实际 macro 实例数、空转跳过数和回退原因。不能把普通 GPU 收益算成 Cell 收益。控制 GPU worker 在 joint lane 开始前释放。
- 加速比默认用**同提交协议的 CPU vs GPU**，且要求完整原始事实、值、等价关系、可见性检查通过；失败时不打印有效加速比。Expr 数量、match 数量、TCPair 代价相同都不能替代这一门槛。
- 真正的提取质量检查调用生产 Tiger，再比较它输出的选定 DAG。不能把 TCPair 或树大小称为实际 extraction 质量。若加上 Tiger 计算比值，说明仍不含序列化/诊断，并单列共享初始化。
- `run --all-rules --joint-sat` 在普通 Run 中默认使用 Cell 的完整绑定查询，不替换整组规则的执行器：Cell 绑定和剩余 GPU 绑定一起按原排序提交全部 RHS。`CELL_DISABLE_CELL_QUERIES=1` 可单独关闭这一复用；旧 `CELL_EXPERIMENTAL_RUN_PROGRAMS` 已废弃。报告 `cell_query_rules`、`cell_query_bindings`、每条规则的 `cell_queries` 和 `dispatches`，不要把查询数称为 kernel 启动数或实际写入数。联合饱和处理仍只在认证边界执行。
- `audit --all-rules --joint-sat` 核对所有查询；加 `--cell-only` 则只核对 Cell 提供的绑定，返回 `audit_scope`，不能拿它当整个 GPU 后端的审计。被饱和宏代替的查询仍由独立整图及 joint 回归覆盖。原生融合、同提交协议 CPU、GPU 的整图与 Tiger 门槛分开报告；提取脚本同时生成 `quality.json` 和 `cpu-gpu-quality.json`，保留原有失败退出码。
- `audit` 检查同一执行状态上的增量查询，既会扰动引擎，也不证明独立运行的最终图相等。当前这个入口检查**原策略**；它不是稳定策略完整运行的替代认证。
- Sat skip 只有真实认证边界才记收益。Math sat-skip 原型不等于 raytrace 已认证；不把普通 GPU 收益算到 Cell，也不把逻辑 match 算成提交的 mutation。
- `parameterized` 是实际原生提交上的 CPU 正确性实验：一份 Bop spine 接线及七个规则程序复用不同 Term/代价，逐阶段核对两条 CPU 对照的完整图和同提交协议的匹配计数。它不使用 GPU，不构建具体闭包目录，也未证明整个 terms 族的饱和跳过或性能收益。失败与有限 Run 语义由 `test` 中的 `parameterized_cell` 回归检查。
- `transition` 是很小的实际运行时消融：同一 p 的两个 producer、外部候选改善 x 后更新 p/q、重复改善、输入 union 和 TCPair 从单一到多个别名。逐阶段核对原生同提交协议及复用开关两侧的完整图和提交绑定数，报告索引读取次数。不开 GPU、不测 Pass-3 加速比；优先用它验证端口接续，再决定是否需要完整计时。输出含独立可运行的 `program.egg`。
- Bake 文件的冷启动验证已纳入 `test` 的 `transfer_artifact`：原生 Analyze 导出、删除训练源码、独立进程加载，以及逐阶段整图与未见输入对照。需要检查/复现文件格式时读 [TRANSFER_FILE.md](../../experiments/parameterized_cell/TRANSFER_FILE.md)，不要重新编写采集器，也不要因此重跑完整 Pass 3。加载时的 `program_compilations=0` 仅指没有重新搜索接线计划，原生 schema/action sink 仍需安装。
- `transfer-bench` 使用已有转移文件，不重新 Bake；默认 512 组同一多候选更新样例，可用 `--instances` 改变规模。三条路径各执行一遍，分别统计 setup、输入处理、推理时间，并在计时外逐阶段核对原生完整图。不是 GPU 或 Pass-3 benchmark。已有结果用 `report` 复用；相同源码 dataflow 与文件加载的对照用于判断加载开销，joint 另有局部证书优化，不能混作纯加载消融。
- `joint-sat` 检查共享 CPU/GPU 运行时的联合闭包宏：新 producer DAG 的边界契约满足时跳过三个 terms ruleset，更新/别名/删除等不满足契约的状态保留完整回退。必须核对每个公开边界的整图，并检查前两个新 Cell 阶段的原始 smooth 匹配为零。`--snapshot FILE` 只验证该快照初始事实上的 terms 子调度，不冒充整个 Pass 3，也不要求它一定命中宏；必须报告实际命中和回退原因。
- 保留失败的报告、输入/worker/binary 指纹和实际命令。看保存结果必须说“未重新测量”。不部署网站、不采集大 trace、不自动重新 Bake。

可重复性和正确性是两件事。不要声称原生 egglog 无条件保证“同输入/二进制/线程数逐字节相同”，也不要要求 GPU 模仿原生所有匹配顺序。现有严格整图检查是回归检查口径，并非 egglog 的通用语义规范；放宽为可观察结果检查时，必须先定义哪些辅助表不再可观察、仍能保证后续规则/提取不受影响，另标检查模式，不能把旧失败结果直接改成通过。普通 Run 的阶段边界、顺序敏感 merge、失败行为不能用“不确定性”一笔带过。输出等价性、提取质量与独立重放确定性分别报告。

差异定位、sat-skip 小样例和报告字段的细节只在需要时读 [references/modes.md](references/modes.md)。交付给用户以结果、限制、报告链接为主；复现时给这个统一入口即可，不再反复展开整段底层命令。

仅在 `CELL_QUERY_TIMINGS=1` 诊断模式下，查询耗时按 ruleset 保存在 `gpu.query_costs_by_ruleset`：`query_seconds` 包含交接和解码，`device_seconds` 来自 worker；两者不能相加。此诊断还将 GPU command buffer 拆开测量 `maintenance_seconds` 与 `matching_seconds`，分别是表/索引维护和匹配阶段（含候选路由）；不以这个模式的总时间发布加速比。维护时间记在随后消费数据的 ruleset 下，不代表该 ruleset 制造了这些更新。用这份分解决定下一项优化，不从逻辑 match 次数推算 probe 成本。

`CELL_SHARE_IDENTICAL_QUERIES=1` 是默认关闭的整条 LHS 查询共享实验；只有完整条件、binding 接线和原生增量游标一致才共享，RHS 不合并。其当前测量并未证明净加速，不把 `shared_query_requests`（含空结果）当作 kernel 启动数或写入数。

完整 GPU 路径现在默认按 Join 层次执行数值条件，并让同一搜索路径复用一份 binding。`CELL_COMPACT_BINDINGS=0` 只关闭紧凑工作区；`CELL_STAGED_CONSTRAINTS=0` 恢复动态条件检查，并对含 primitive 的程序关闭紧凑工作区。用同一个二进制进行消融，记录 `gpu.cells.staged_constraints` 和 `compact_binding_workspace`。这是匹配器工作区优化，不是新增 Cell 覆盖或图存储压缩。新 shader 的冷初始化可能很慢，必须同时报告 `setup_seconds`，不要把排除初始化的推理收益说成冷启动总收益。分项实验与拒绝采用的改动见 [INDIVIDUAL_OPTIMIZATIONS.md](../../experiments/pass3_full_gpu/INDIVIDUAL_OPTIMIZATIONS.md)。

联合 Cell 现在默认在已验证的 uniform-cost 条件下，批量关闭内部 `Smaller` 比较 Saturate。普通 Run 不变；外部 merge、容器键或代价别名不满足证明时回退。`CELL_COMPARISON_CLOSURE=0` 做同二进制消融。报告 `gpu.saturation_diagnostics.joint.comparison_closure` 的实际使用次数、`native_batches` 和 `union_proposals`；后者不是新增事实数。不能因外层 12 次仍显示 dataflow、fresh-DAG 宏为零而漏报内层闭包，也不能把内层收益宣称为整个 Cell 一次回放。结果见 [SAT_COMPARISON.md](../../experiments/pass3_full_gpu/SAT_COMPARISON.md)。

紧凑 GPU Join 默认使用自动证否 lookahead；`CELL_REFUTATION=0` 关闭。编译器只选择已有 tuple index，提前检测未访问 atom 的必要键，空哈希桶可直接拒绝整条后继搜索。碰撞及退休行只可能多保留候选；union/rebuild 仍使用原生完整增量维护，不以 Cell 库未覆盖作为证否。它不是新增 Cell 闭包证明。

后续版本默认再检查每桶 32 位键指纹；`CELL_REFUTATION_SIGNATURES=0` 恢复上一版仅查空桶。`CELL_REFUTATION_DOMAINS=1` 开启可选的共享中间取值求交，默认关闭。`CELL_REFUTATION=0` 同时关闭三种过滤及摘要维护。报告 GPU 摘要字节数，不能称为 RSS 或压缩率；计数中的 `signature_refuted_branches` 已包含在 `refuted_branches` 内，不能相加，`domain_refuted_branches` 才是另一组排除。当前 worker layout 17，旧 worker 需重建。见 [共享桶摘要](../../experiments/automatic_refutation/BUCKET_SUMMARIES.md)。

从 layout 13 起，`CELL_SEARCH_COUNTERS=1` 另记录多 atom 查询的起始候选数 `eligible_seeds`、当场被否的 `refuted_seeds`、至少产生一条完整 binding 的 `productive_seeds`，并按原始 rule 汇总。命中率用 `refuted_seeds / eligible_seeds`；不能将包括深层分支的 `refuted_branches` 除以起始候选数。无剩余候选时比率未定义。普通运行不执行这些诊断 atomic，也不分配按 rule 的计数区。

互补性实验用同二进制、同输入的 2×2 组合；S 为 `--joint-sat` 的 CPU Cell 服务，R 为 `CELL_REFUTATION`。中间组另设 `CELL_DISABLE_CELL_QUERIES=1`，区分纯饱和接续与普通查询复用。`interaction` 只读已有目录，校验配置/指纹/整图/计数守恒，比较固定规则权重下的证否率，不能把移走查询导致的比例升高当成同一候选的证明增强，也不能把这些插桩时间报成加速比。`--output NEW.json` 保存分析，已有文件不覆盖。无需重跑原生/CPU；同调度的 joint 对照也可为不启用 hook 的候选提供原生/CPU 图，旧 joint 判等字段会被删除后重新核对。见 [互补性验证](../../experiments/sat_refutation_interaction/README.md)。

比较 search 必须用同输入、同调度、同二进制开关消融。`CELL_QUERY_TIMINGS=1` 分离 `gpu_matching_seconds`（含证否和候选路由）与 `gpu_maintenance_seconds`；不要把该诊断的总推理时间作为正常运行加速比。另用普通 `run` 报告完整推理耗时。`CELL_SEARCH_COUNTERS=1` 在全规则路径开启诊断计数，报告 `join_probes`（实际进入的索引 Join 层）、`refutation_checks`（额外的必要键检查）、`refuted_branches`、`join_link_visits`（含退休行）和 `join_live_row_visits`（实际候选访问）；不是新事实或唯一行数。计数器含 atomic 开销，不作为无插桩时间证据。修改证否后须跑 `audit --all-rules --joint-sat`，并检查新增事实、union、删除重插和精确整数键。见 [自动证否实验](../../experiments/automatic_refutation/README.md)。

Layout 14 的 `join_depth_fields` 定义逐层计数字段，`rules[].stats.depth_work` 保存每条规则的数据。0 层是已通过 anchor 检查的多 atom 起点，d>0 是之后第 d 个 atom 的 Join 尝试；不是表达式树深度或 ruleset 轮次。记录循环进入、索引链/活行访问、绑定成功的 prefix、证否、完整匹配叶子等。`deepest_seeds` 每次 seed 执行只在其最大尝试深度计一次，用来计算 Pr(max_depth >= d)。分叉会让 `entries[d]/eligible_seeds` 大于 1，不能把它当概率。

`depth` 只读已保存的四组计数，校验逐规则的下一层进入数 = 本层 prefix − 本层证否、最深层直方图与 seed 总数、以及全局行访问/证否计数，并输出固定规则权重的曲线。`--output`/`--csv` 不覆盖既有文件；不重新编译或推理。无完整深度 schema 的旧记录不能用来推造概率。普通执行不分配这些逐层诊断计数区，且不执行诊断 atomic。可视化和复现见 [Join 深度分布](../../experiments/join_depth_distribution/README.md)。

注意旧 GPU `join_probes` 在输出缓冲区重试时重置，记录每个 packet 最后一遍；逐层 entries 与 `join_live_row_visits` 包含所有尝试。总循环进入使用 `compact_probe_attempts`，并保留 `legacy_final_attempt_probes` 作口径对照，不能强行判它们相等。这不是原生 CPU probe 计数的修改。当前逐层统计限 compact GPU 执行器，插桩时间仍不作为加速比。


Cell 完整接口实验：`CELL_BINDING_CONNECTIONS=1` 开启已安装参数化 Cell 程序的完整函数键连接（空集／唯一 binding）；`CELL_BINDING_RANGES=1` 开启同一来源的数值端口范围。均为实验开关，默认关闭，不能仅凭减少扫描便启用默认路径。范围按完整原生索引 fiber 更新，包含全部当前候选，并允许哈希碰撞／退休行扩大范围。它只能证否，不能据两个边际范围拼出真实 binding。i64 在有界整数坐标中保守处理超界值，保留原生精确整数 Join 作最终验证。

比较这些接口时，固定 `--joint-sat`、普通 Cell 查询分流和原有 refutation 开关，使用同二进制的开关消融。`depth-pair` 校验整图、调度、二进制、worker、逐规则起点与产出数、分层守恒，并报告 `connection_*` / `range_*`；它不把“起点数量相等”说成逐个起点身份完全相同。`connection_unknown` 是失效／未覆盖后回退；完整范围采用可单调放宽的上界，删除后不缩小，重建时重新收紧；新事实和 canonical row 必须先纳入当前 packet 再查。

实际 GPU 快速回归：设置 `CELL_MATRIX_WORKER` 为当前 worker，`CELL_BINDING_CONNECTIONS=1 CELL_BINDING_RANGES=1 CELL_SEARCH_COUNTERS=1`，运行 `cell-assisted-egglog` 的 `full_gpu` 测试；必须覆盖缺失键后续出现、函数更新、union、subsume、索引 rebase 和 i64 极值。完整 `audit --all-rules --joint-sat` 与整图检查仍不可替代。来源局限：当前导出的是已安装参数化 Cell 程序的端口，不是任意 Analyze 闭包的完整关系，也没有证明全部 Cell 形变都已被支持。


`cell-cases` 按已安装参数化 Cell 程序统计，而非学习目录里的 e-class，也不是每个实例的独立历史。需要 telemetry v2：报告须包含 scalar 程序，缓存读/索引读分母和非空回答次数。旧报告缺字段时不能用 `emitted_bindings / seed_evaluations` 冒充缓存命中率；必要时只补一次诊断，历史关/开对照继续复用。脚本要求历史对照同二进制，并核验新诊断保持原来的逐规则工作量及整图；不将补采时间用作性能证据。

三种比例分开：端口缓存率 = 缓存读 / (缓存读 + 新索引读)，包括已知缺失端口；非空回答率 = 非空 Cell 查询回答 / Cell 查询请求；查询接管率 = Cell 完整回答 / 该规则全部请求。没有端口读取时缓存率为 N/A，不是 0%。空回答可能避开大量无用起点，缓存率 0% 也可能通过固定接线与依赖订阅省下 Join。另列 fresh-DAG 宏、参数化 dataflow 和内部比较闭包的计数，不能把它们合成“整 Cell 命中”。单条规则的 GPU 工作移除不是该 Cell 的独立耗时贡献。


`cell-value` 只读已有记录，按程序列出移除起点、移除活候选行及其占整个基线的差额份额，同时保留局部工作账与未归属差额。空查询可以只省起点；缓存率为零仍可观察到工作迁移；回退增多可产生负差额；零分母不造百分比。它是全开/全关后的工作归属，不是 Cell credit，不是删掉某一个 Cell 的独立因果贡献，也不能把 CPU 根评估、端口读取、GPU 行访问相加相减成“净操作数”。

整套执行方案是否默认启用可看**净推理耗时降幅** `(T_off - T_on) / T_off`，计入运行期维护、交接与原生 effects，setup/Bake/extraction 单列。`cell-value --timing-before DIR --timing-after DIR` 仅接受同二进制、同 worker、同输入/调度、原有过滤开关一致、整图通过且无插桩的 Cell 关/开对照。没有这样的数据就报告未测，不拿旧 GPU 内核或插桩秒数凑加速比。这只能评价整套服务，不能用作 Cell 选择 credit；后者使用下面保留公共执行器的对照。单个 Cell 的边际时间收益须用 `T(库去掉 C) - T(完整库)`，当前未采集，不把全开/全关的规则计数当它的替代。


`CELL_JOINT_PROOFS=0/1` 提供同一参数化执行器上的证明组消融：保留普通 scalar 查询接管、固定根计划、订阅、端口缓存和原生 effects；只改变 fresh-DAG 宏、uniform-cost 退役及 Smaller 比较闭包，并包含其维护成本。两组都用 `--all-rules --joint-sat`。`cell-credit` 核验该契约、相同二进制/worker/输入/调度/查询接线和无插桩整图门槛；拒绝用“去掉整个 handler”的实验冒充此对照，也拒绝没有实际消费证明的时间差。

测到的 credit 只归 `terms-joint-closure-proofs` 这一组，不能拆给三个 Smaller 规则、其父 Cell 或等价 spine 重复记账。当前尚无一般的逐 Cell 证明消费账和独立边际 credit。复用旧 CPU/完整图控制仍允许；报告中的历史 `gpu_without_joint` 可能来自别的 GPU 版本，因此不再自动据它显示“joint 单独收益”，正确归因须读独立同版本关/开记录。


GPU 后端从 layout 16 起默认启用必需表为空的批次短路。`EGGLOG_GPU_EMPTY_TABLE_PRUNING=0` 关闭作消融；它归 `gpu_optimizations.empty_table_pruning`，不归 Cell。worker 在处理本批全部增量后的可见行计数上判断，重插/union/rebuild 后重新检查；零 atom 的规则不被短路。整批为空也继续提交维护并推进请求游标。`gpu_empty_table_prunes` / 每规则 `empty_table_prunes` 统计被短路的请求，是原 `dispatches` 的子集，不是内核启动数，也不能直接当作省掉的起点数。

`empty-tables` 比较同二进制的无 Cell 运行，只改变这个开关，保留全部规则/调度和原生 effects；统计用 `CELL_SEARCH_COUNTERS=1`，时间另取无插桩对照。使用已验证 CPU 控制，不重跑未变的 CPU。要对照收益按原深度字段核算，不能把被短路请求乘表行数估算。Cell/refutation/credit 的其它对照必须固定此通用开关；工具会拒绝混用。空表短路没有 Cell credit。该优化只处理整个必需表没有可见行，非空表之间连不上仍由 Join/约束处理。

## 自动 Analyze 库的独立对照

`run/audit --all-rules --analysis DIR` 加载普通 Analyze 的 `catalog/`，与源码编译的 joint observer 分开。`--analysis-plan-only` 保留同库引导的查询顺序和后端，但不启用片段认证/复用；必须同时指定 `--analysis`。公共 CPU dataflow、joint 证明、GPU 通用优化保持一致。库中的所有 catalog/state 文件内容进入缓存指纹，目录名不代表库内容不变。

使用三组同二进制运行：无库、同库 plan-only、同库开启复用。`analyze-credit --baseline DIR --plan-only DIR --candidate DIR [--profile-plan DIR --profile-candidate DIR] --output NEW.json` 核验后分开报告库的总净收益及顺序/片段复用的条件差值。时间不含 setup、Bake、extraction，均须另列；不能拿源码 terms 证明的收益冒充该库收益。复用未变的 CPU 控制；每组只跑一次。

当前接线仅复用已认证的正向、全键结构片段。含 computed primitive 的查询逐条保持完整 GPU 匹配，其他查询保留紧凑工作区/refutation。片段第一次认证与订阅维护不免费；报告 loaded states、共享计划、实际消费者、认证读取、普通/port 探测、watch 检查和 GPU 分配字节。13 条投影查询的旧执行器尚无完整逐层行数，最新诊断曲线只覆盖其余 compact 查询，不能冒称完整工作量。冻结的 If/Single 库在完整 Pass 3 的一次测量为负收益，见 `experiments/analyze_library_credit/README.md`；它不是完整 Pass-3 Bake 库的结论。

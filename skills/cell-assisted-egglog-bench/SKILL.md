---
name: cell-assisted-egglog-bench
description: 运行和解读 cell-assisted-egglog 的 CPU/GPU、稳定选择、整图一致性、Tiger extraction、增量查询 audit、差异定位和 sat-skip microbench。用于 egg_layout 的辅助运行时验证与性能对比；eggcc 原生三 pass 的 Saturate/Run 耗时统计使用 eggcc-pass-benchmark。
---

# Cell-assisted egglog 验证与计时

使用本 skill 的 `scripts/bench.py`，不要重新拼 Rust、Swift、Tiger 命令或另写统计器。脚本默认使用本 skill 所属的版本化 checkout，`--repo` 可指定其它 checkout；会显示实际路径。原始快照和生产 Tiger 默认来自共享的 `Repos/egg_related`，不假设 worktree 旁边有 check_statewalk。

`$SKILL` 指本目录。用 `/opt/homebrew/bin/python3`，或其它 Python 3.9+。

## 默认复用历史

按用户约定，假设同一台机器的性能稳定。**先读已有结果，再决定是否执行。** 不为刷新日期、增加平均次数或陪跑未改的对照而重测；也不要重复解释已经固定的底层命令。

- 先用 `history --joint-sat` 找同输入、同选择策略、同实验模式的已通过记录，再 `report 目录`。去掉 `--joint-sat` 查普通 GPU 对照。按最近的适用版本选择基准，不挑最快一次。
- 未改变的 CPU、GPU-off、输入准备、Tiger 结果可作为历史对照。保留其目录、版本、配置和时间，不把历史时间写成这次新测。候选实现改变时，旧结果不能证明新实现正确。
- `run` 在增量构建后，若输入、实际 Rust/worker 二进制、策略、joint 开关、记录的运行配置和机器一致，会直接返回已验证结果；不重新推理、不创建新的 output 目录。需要 Tiger 时，还核对提取工具与参照输出的指纹。`--refresh` 才强制重测。
- 完整 `run --all-rules` 会保存原生、同提交协议 CPU、GPU-off 的完整事实及冻结输入锚点。后续用 `run --all-rules --joint-sat --reuse-controls 旧目录` 只运行新 GPU 候选，重新核对完整事实、值、等价关系和可见性；旧时间明确标为历史。要求输入、调度、分区和选择策略相同，且旧 CPU/GPU 图检查通过。旧目录缺少 `graphs/*.control.json` 时不能使用此模式。
- 缓存校验使用完整结构比较，不以哈希代替判等。native/CPU 的 Tiger 结果只在提取器和输入/输出指纹均一致时复用；新 GPU 的提取与检查仍执行。只读询问仍用 `report`，不调用 `run`。
- 基线二进制、输入/调度、线程/worker、选择策略或验证口径改变时，先判断哪部分对照受影响；只有受影响的结果才需要刷新。公共内核变化可能同时影响 CPU 和 GPU。

## 常用入口

| 需求 | `bench.py` 子命令 |
|---|---|
| 看已有结果，不重新跑 | `report [分析目录或 report.json]` |
| 查找可复用的完整 Pass-3 历史 | `history --joint-sat` |
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

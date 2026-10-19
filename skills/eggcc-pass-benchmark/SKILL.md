---
name: eggcc-pass-benchmark
description: 统计 eggcc 真实三 pass 的执行、search、RHS、insert/merge、rebuild、extraction、Cell 覆盖率，以及 Saturate/普通 Run 调度耗时和占比；也可只读汇总已有结果。用于生产 eggcc 分阶段 profiling，CPU/GPU 辅助运行时对比使用 cell-assisted-egglog-bench。
---

# eggcc 分 Pass 统计

默认仓库 `/Users/mineralsteins/Repos/egg_related/egg_layout`，生产 eggcc 在旁边的 `check_statewalk`。先用本技能的脚本，不重新研究 profiler、LLVM 配置或编写另一套统计器。

## 最短路径

`$SKILL` 表示本 SKILL.md 所在目录；用 `/opt/homebrew/bin/python3` 或 Python 3.9+，脚本也支持 `--repo`。为保持已有缓存兼容，原覆盖统计入口默认原仓库；新增 `saturate-*` 默认本 skill 所属的版本化 checkout。两者都会打印实际路径，`--repo` 可统一覆盖。

## Saturate / 普通 Run 统计

直接使用同一个 `scripts/bench.py`，不要重新修改 profiler 或复制命令序列：

| 需求 | 子命令 |
|---|---|
| 只读现有 raytrace 三 pass 调度统计 | `saturate-report` |
| 汇总其它保存结果 | `saturate-report --input 分析目录` |
| 核验 schedule profiler 和输入 | `saturate-check` |
| 单次测量真实 raytrace 三 pass | `saturate-run --output out/新目录` |
| 换程序或指定已有 profiler | `saturate-run --source /绝对路径/程序.rs --profile-binary /绝对路径/eggcc-schedule-profile --output out/新目录` |
| 缺 schedule profiler 才构建 | `saturate-build --output out/新profiler目录` |

`--source` 可重复；`--production-repo` 可更改生产 checkout，默认仍是共享的 `check_statewalk`，不是 managed worktree 的兄弟目录。`--dry-run` 放在子命令前只展示命令。

此模式调用已有 `build_profile.py --schedule-only` 和 `run.py --timing-only`。**不需要冻结库、flatten、coverage、analyze 或 Bake**。保持真实的 pass→Tiger extraction→下个 pass 输入链。构建只修改/恢复生产 pass driver，不改引擎；不要并发编辑/构建生产目录。缓存指纹和实际采用的路径会显示，缓存存在不代表源码新鲜。

- 只对最外层 Saturate 计整块时间，内部嵌套时间不能重复相加。普通 Run 只统计 saturation 外的独立调用。
- 分别报告 `Saturate / scheduled execution`、`Saturate / execution including setup`、`Saturate / CLI wall`；extraction、serialization 单列。
- Native Repeat 与 production experimental `run-schedule` 的 repeat 停止语义不同；保留原调度，不能手工换成 core AST 后称为生产计时。
- profiler 校验各 packet 的 match、search/apply、merge、rebuild 总和与 native totals 一致。没有有效 `schedule_profile` 是缺失，不能报成 0 秒。
- Saturate 时间多不代表已获得 Cell 闭包证书；不能直接换算 sat-skip 加速比。这里的 search+apply 包括 RHS，没有独立 RHS 计时就不要拆出猜测数字。

输出：`saturation-summary.json`、各 workload 的 `pass-{1,2,3}.json` 和报告。读取旧结果必须说明未重新测量；`saturate-report` 不覆盖保存的文件。

## 执行与覆盖率统计

- **看已有表／解释结果**：
  `python3 "$SKILL/scripts/bench.py" report`
  默认读已提交的 `experiments/eggcc_passes/results.json`，明确告诉用户这是报告所标日期的保存结果。
- **重新测量**：先运行 `python3 "$SKILL/scripts/bench.py" check`；工具齐全后直接
  `python3 "$SKILL/scripts/bench.py" run`
  默认 fact、loopfact，各完整运行一次三个 pass，不求平均。输出写到新的 out/ 子目录。
- **重绘其他结果**：
  `python3 "$SKILL/scripts/bench.py" report --input out/某次运行/summary.json`

- **指定样例**：`python3 "$SKILL/scripts/bench.py" run --source ../check_statewalk/benchmarks/passing/raytrace/raytrace.rs`
  `--source` 可重复。使用 `optimized-pretty-print`，与 `dag-optimize` 调用同一优化流程，但不执行 Graphviz；不要把画图耗时算成编译推理时间。

不要因为“重新统计”就重建所有工具、重新 analyze、重新采集 trace 或 bake。覆盖率使用现有冻结库。用户要求重新 bake 或不同样例时，明确扩展现有 runner，而不是把默认两个样例当成不可更改要求。

## 缺依赖才读这里

`check` 会列出缺失项。只在缺工具、构建失败或用户要求最新源码时读 [references/build.md](references/build.md)。默认复用指定版本的缓存工具；存在二进制不等于与当前源码一致。若要求最新版本，核对来源或重编译。

## 必须保持的统计口径

- 真实生产链：前一 pass 做 Tiger extraction，结果传入下一 pass 的新 egraph。不能独立把原始程序跑三次充当三个 pass。
- 计时使用生产 eggcc 的插桩二进制；覆盖率是另一轮本地诊断，不混入计时。
- `execution_ms` 包含解析、类型检查、安装等准备工作；不是纯推理时间。
- `search_and_binding_ms` 是 search+apply 减去 RHS 指令计时，仍含匹配管理。
- `rhs_ms` 包含 primitive、构造器查找、写缓冲；`insert_merge_ms` 是提交插入和 merge 的合计，不能伪造单独 insert 时间。
- 主表用 `extraction_wall_ms`；提取器内部计时和序列化保留在 JSON。不要把序列化算进 search。
- 默认库是旧 fact Pass-1 的 163 个观察模式条目，不是每个 pass 新 bake 的认证 Cell。fact 为训练内，loopfact 为 held-out；Pass 2/3 是旧库迁移覆盖。
- 表里同时显示 Expr 节点、Expr e-class、constructor 行及完整 LHS 覆盖。节点覆盖不等于免 Join。
- 覆盖测试必须 `same_final_state_as_native_control=true`，冻结签名数量不变。保留分母、source/library hash，不把各 pass 的图合成一个分母。

## 交付

覆盖模式给时间、覆盖率两张表；Saturate 模式给各 pass 的 Saturate/Run/提取时间和热点调度块表，不为凑覆盖率重新跑 analyze。附报告路径，说明版本、库口径与未拆分计时。报告复用不能冒充新测量。复现优先给统一 skill 入口，不再重复展开底层构建/运行命令。

---
name: cell-assisted-egglog-bench
description: 运行和解读 cell-assisted-egglog 的 CPU/GPU、稳定选择、整图一致性、Tiger extraction、增量查询 audit、差异定位和 sat-skip microbench。用于 egg_layout 的辅助运行时验证与性能对比；eggcc 原生三 pass 的 Saturate/Run 耗时统计使用 eggcc-pass-benchmark。
---

# Cell-assisted egglog 验证与计时

使用本 skill 的 `scripts/bench.py`，不要重新拼 Rust、Swift、Tiger 命令或另写统计器。脚本默认使用本 skill 所属的版本化 checkout，`--repo` 可指定其它 checkout；会显示实际路径。原始快照和生产 Tiger 默认来自共享的 `Repos/egg_related`，不假设 worktree 旁边有 check_statewalk。

`$SKILL` 指本目录。用 `/opt/homebrew/bin/python3`，或其它 Python 3.9+。

## 常用入口

| 需求 | `bench.py` 子命令 |
|---|---|
| 看已有结果，不重新跑 | `report [分析目录或 report.json]` |
| 核验输入、工具与缓存路径 | `check` |
| 真实 raytrace Pass 3 稳定选择计时＋整图检查＋Tiger | `run --output out/新目录` |
| 只跑推理、推迟 Tiger | `run --output out/新目录 --no-extract` |
| 补做已有图的真实提取检查 | `extract 分析目录` |
| 重跑未改 `merge new` 的对照 | `run --policy original --output out/新目录` |
| 原策略 GPU 增量查询 oracle | `audit --output out/新目录` |
| 原策略 CPU 自对照 | `cpu-pair --output out/新目录` |
| 原策略首次差异定位 | `diff --output out/新目录` |
| Math 的认证饱和跳过实验 | `sat-skip --output out/新目录` |
| 相关正确性回归 | `test` |

```sh
/opt/homebrew/bin/python3 "$SKILL/scripts/bench.py" check
/opt/homebrew/bin/python3 "$SKILL/scripts/bench.py" run --output out/raytrace-stable-new
```

执行类入口默认离线增量构建所需 Rust 示例；Swift worker 以源码/二进制指纹复用。构建不计入报告中的推理时间。仅明确复用已核验二进制时使用 `--no-build`；它不意味着缓存一定新鲜。全局 `--dry-run` 展示实际命令，不构建、不测量、不写输出。

默认是完整**已保存 Pass-3 输入和主调度的回放**，不是重新执行 eggcc 三个 pass，也不是 pristine upstream。允许 `--snapshot` 指定相同格式的其它完整快照；当前辅助分区固定为三个 terms ruleset，不能据此宣称支持任意 `.egg`。缺快照就报告缺失，不换小 If/Math 冒充 raytrace。

## 验证和计时的界限

- 单次运行每条路径，不自动取平均。不要同时编译、audit、做图或运行其它 benchmark。
- `run` 的稳定策略在 CPU/GPU 共用同一原生 merge；主计时包含结构键维护，输入锚点准备和 worker setup 单列。脚本在整图失败后仍可做 Tiger 检查，但保留失败退出码。
- 加速比默认用**同提交协议的 CPU vs GPU**，且要求完整原始事实、值、等价关系、可见性检查通过；失败时不打印有效加速比。Expr 数量、match 数量、TCPair 代价相同都不能替代这一门槛。
- 真正的提取质量检查调用生产 Tiger，再比较它输出的选定 DAG。不能把 TCPair 或树大小称为实际 extraction 质量。若加上 Tiger 计算比值，说明仍不含序列化/诊断，并单列共享初始化。
- `audit` 检查同一执行状态上的增量查询，既会扰动引擎，也不证明独立运行的最终图相等。当前这个入口检查**原策略**；它不是稳定策略完整运行的替代认证。
- Sat skip 只有真实认证边界才记收益。Math sat-skip 原型不等于 raytrace 已认证；不把普通 GPU 收益算到 Cell，也不把逻辑 match 算成提交的 mutation。
- 保留失败的报告、输入/worker/binary 指纹和实际命令。看保存结果必须说“未重新测量”。不部署网站、不采集大 trace、不自动重新 Bake。

差异定位、sat-skip 小样例和报告字段的细节只在需要时读 [references/modes.md](references/modes.md)。交付给用户以结果、限制、报告链接为主；复现时给这个统一入口即可，不再反复展开整段底层命令。

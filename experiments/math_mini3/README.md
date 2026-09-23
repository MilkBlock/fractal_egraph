# math mini3：时间与内存诊断

2026-09-23，基于 `4037735`，另加 SVG 生成开关用于对照。`source.egg` 仅把原始 `egglog/tests/math-microbenchmark.egg` 的 `(run 11)` 改为 `(run 3)`，所有规则与初始表达式保持原样。

## 单次测量结果

| 模式 | 用时 | 峰值 RSS | 结果 |
|---|---:|---:|---|
| 原生，无 trace | 原生执行 **5.39 ms** | **8.17 MiB** | 3 轮完成，208 条 constructor 行 |
| 默认分析，自动 SVG，无 ripen 上限 | ≥10 s | 采样 278 MiB | 外部时间保护停止 |
| 50 ms / 200K，自动 SVG | ≥30 s | 采样 538 MiB | 外部时间保护停止 |
| 50 ms / 200K，不自动 SVG | **6.62 s** | **1.55 GiB** | 分析结束；79 Saturated、6 ObservedUnfinished |
| 无 ripen 上限，不自动 SVG | ≥10 s | 采样 **3.55 GiB** | 外部时间保护停止 |

每配置只运行一次。默认分析、带上限且自动 SVG 另有初始 10 秒观察记录，全部原始数字见 `result.json`。原生执行时间来自进程内计时；分析时间为外部总时间，不能直接拿它们计算精确加速比。完成进程的峰值用 `/usr/bin/time -l`，被终止进程使用约 0.1 秒间隔的进程组 RSS 采样，因此是下界。

所有分析测试有外部 **10/30 秒、4 GiB RSS** 保护（区别于每个 ripen 的 50 ms / 200K）。没有复现到 25 GB；没有为了追到该峰值继续占用机器。

## 已确认的原因

1. **三轮只限制 tier0，不限制后续 ripen。** 当前默认仍没有时间/size 上限，50 ms 和 200K 必须显式配置。默认无 SVG 的测试在 10 秒窗口内已达到 3.55 GiB，仍未完成。
2. **自动 SVG 阻塞热路径。** 栈采样发现原生 `to_svg_file`，包括 `snapshot_unfinished → export_state → to_svg_file`。暂停时本来应只保存 unfinished 状态，实际还在等待 Graphviz。这部分不受 ripen 的时间/size 阈值保护。关闭自动 SVG 后，同一个带上限 mini3 能完成。
3. **关闭 SVG 后，Rust 分析层仍占用很大。** 1.55 GiB 的峰值属于 `egg_layout`，不是浏览器或 dot。mini3 只有 208 条 tier0 constructor 行，却派生了 85 个 ripen 任务；未完成 cell 中最大的观察图有 13,678 条事实。
4. **全量 binding 和多份报告也是实际开销。** 完成运行保留 626 条 weak 关系、50,552 个 binding tuple。队列 JSON 约 13.7 MiB，其中 weak 证据序列化约 7.37 MiB，桶触发匹配约 4.98 MiB；相近内容又写入多个 round 与 analysis。采样同时出现 native Join 与序列化/克隆路径。不过这些数字不能精确解释全部 1.55 GiB，还没有按分配来源做完整内存归因。

关闭 SVG、配置上限后的阶段累计：准备约 0.44 s，推进 ripen/共享约 2.28 s，精确目录约 0.12 s，weak 查询与证据约 1.85 s；剩余时间包含报告与文件操作。不要再将所有耗时归因于 coarse facts 构造。

## 可重复的受保护测试

```sh
cargo build --release --offline --bin egg_layout --example native_math_baseline
python3 experiments/math_mini3/bench.py --output out/mini3-new
# 只测配置 50ms/200K 且不自动生成 SVG
python3 experiments/math_mini3/bench.py --output out/mini3-nosvg-new \
  --bounded-only --no-svg
```

直接运行（不含外部整进程保护）：

```sh
EGG_LAYOUT_EAGER_SVG=0 cargo run --release --offline -- analyze \
  --source experiments/math_mini3/source.egg --output out/mini3-analysis \
  --ripen-observe-ms 50 --ripen-observe-facts 200000
```

`EGG_LAYOUT_EAGER_SVG=0` 仅关闭自动 SVG，保留原生 DOT、JSON 和分析；页面已有从 DOT 按需渲染的路径。未设置时仍维持旧的自动 SVG 行为。本次没有偷偷修改默认时间/空间上限，也没有删减规则或推理结果。

原始日志、采样栈和结果保存在 `out/math-mini3-*`。报告是快速诊断，不是统计显著性测试，也不是“25 GB 已经解决”的声明。

## 用户指定进程的现场确认

随后检查 Python PID 10975 的子进程 `egg_layout` PID 63343，`sample` 明确报告 **physical footprint 25.4G**。这与前面受保护 mini3 未达到该峰值是不同的观察。

现场输入实际仍是 `(run 11)`，最后打印完成的 tier0 进度为第 3 轮。该次队列配置是时间/事实数均无上限、每 sweep 最多 4 轮。日志连续记录 2,334,405、8,589,833、33,250,992 条 trace events，以及最后 8,538,587 个 producer 记录。主线程采样全部落在 `Session::advance_with_observation → collect → BTreeMap::retain/ExtractIf`，此时不是 SVG 渲染。

因此已直接确认用户报告的高内存现场：大量 trace 批次和持续保留的 producer/历史正在处理。`drain_completed` 只是把事件从缓冲移交给 collector，不等于这些数据及其衍生历史已经释放。要进一步分摊每一种分配仍需专门内存分析。

只对该子进程发送 SIGSTOP 保留现场，未终止 Python；暂停不释放内存。现场摘要见 `live_process.json`，栈保存在 `/tmp/egg-layout-63343.sample.txt`。PID 状态是当时记录，不保证日后仍有效。

## Native admission protection and compact presentation trace

One actual egglog runtime run (`native_admission.json`), using the **defaults** with no explicit ripen flags and SVG disabled: 6.19 s elapsed, 1,527,922,688 bytes peak process RSS (~1.42 GiB), exit 0. Command: `python3 experiments/math_mini3/bench.py --output out/native-admission-mini3 --default-only --no-svg --seconds 15`. The recorded commit is the parent; measurement included the working-tree patch described in the result.

Default ripen limits are now 50 ms / 200,000 facts. Native join/admission checks stop before additional match trace allocation; admitted effects and rebuild finish. Pending write reservations may stop before actual facts reach the limit. This is not a total-process memory cap. Default presentation omits per-application DOT/Typst stream events; analytical provenance is retained. The remaining ~1.42 GiB is still substantial, and this experiment does not establish a major memory reduction.

## Graphless optimization (actual runtime)

Reproduce the baseline comparison with `python3 experiments/math_mini3/bench.py --output out/graphless-comparison --no-graphs --compare-native --seconds 15`. This does not generate DOT/SVG. `graphless_optimization.json` records one run per code revision, with matching source and observation defaults. Final `/usr/bin/time` wall time: **5.12 s**, compared with **6.32 s** before the changes; native process wall time **0.18 s**, native internal timer **0.005185 s**. Do not compare the internal timer directly as an end-to-end speedup. Compilation is excluded.

Changes: typed scalar export avoids whole-graph serialization, identical typed weak queries share compiled plans within the same native engine, report inspection borrows instead of cloning, and progress/final reports avoid redundant copies. Final phase totals: registration 0.400 s; advance/share 1.498 s; exact catalog 0.136 s; weak queries/evidence 1.790 s. There are still 2,001 compiled query pairs and 2,722 current scans. This is not near native execution speed. Peak RSS was 1,400,029,184 bytes, higher than the pre-change 1,269,071,872 bytes; no memory improvement claim.

All 79 completed state JSONs are exactly equal before/after; six unfinished prefixes remain time-dependent. Time-bound work can grow farther after an optimization; unfinished evidence is not a fixed-work benchmark. Tests cover identical-query sharing, preservation of literal distinctions, explicit payload erasure, and existing CS/ripen regressions.

---
name: eggfuse-metal-profile
description: 用 Xcode xctrace、Metal GPU Counters 和 gpudebug 定位 eggfuse/egg_layout GPU 后端的 kernel 热点、低占用率、寄存器溢出、带宽与 CPU/GPU 等待。复用保存的真实工作负载，生成可复查的诊断；不将 profiler 重放时间当成端到端 benchmark。
---

# Eggfuse Metal profiling

先读已有结果。实现或输入没变时复用 trace、导出和嵌入的 GPU profile，不重跑原生 CPU 对照。需要新采集时使用本目录脚本，避免重新拼协议和 XML 统计器。

## 先选择正确的执行路径

- **当前 resident ABI7 完整运行**：使用 `resident_full.py` 给保存的 `attempt-*/program.json + result.json` 做全程 encoder 时间戳。保留原 quantum 和私有多阶段执行；复用同输入、同 worker 的无插桩基线，逐字段核对完整状态、历史、私有区域及外层提交数。用 `resident_window.py` 在同一次完整执行里采多个硬件窗口，避免为每个窗口重放前缀。完整流程和字段口径见 [resident 精确测量](references/resident-measurement.md)。
- **单 Run / 快照诊断**：原 `resident.py --mode capture|timings|encoder` 仍可用。精确 stop 会将私有区域展开为普通 Run；旧 timing/encoder 模式还固定 `quantum=1`。它们不能代替完整生产路径的计时，也不能据此评价私有多阶段吞吐量。
- **旧 device 后端 DVB6/7**：使用下方 `profile.py` 的已保存 Run 流；不要把 CPU 提交式 assisted matcher 或 resident 输入塞进这个协议。不同后端的秒数必须标明配置。

当前路径推荐：

```sh
python3 "$PROFILE_SKILL/scripts/resident_full.py" \
  --run "$SAVED_ATTEMPT" --build "$BUILD" --output "$NEW_PROFILE"
python3 "$PROFILE_SKILL/scripts/resident_window.py" \
  --run "$SAVED_ATTEMPT" --build "$BUILD" --output "$NEW_COUNTERS" \
  --mode counters --window START:COUNT --window OTHER_START:OTHER_COUNT
```

窗口坐标是**外层 command-buffer 序号，不是逻辑 Run**，从全程 profile 的 `outer_before` 选取。脚本记录实际 Run 范围，等待 xctrace 就绪后才放行 GPU，采集结束后继续跑完整程序并核对终态。不得将这种主动暂停的墙钟作为性能基线。

`--mode capture --window START:COUNT` 生成有界 `.gputrace`，再交给 `capture_report.py`。真实巨型 shader 的 Apple 重放编译可能超过300秒；先区分编译与执行，保留失败，确需延长时显式传 `--timeout-seconds N`。不自动反复采同一窗口。

Capture 先写本机 APFS；本机曾出现直接写 exFAT 的 capture 无法读取，而同窗口写 APFS 可读取。脚本会拒绝非 APFS capture 输出。完成 profile/embed 后，用 `artifacts.py archive --trace LOCAL.gputrace --output EXTERNAL.gputrace.tar` 做校验和归档，保留 capture 内部符号链接和源文件；读取时解包到 APFS。校验和只证明复制一致，不证明 Apple profile 内容有效。硬件 `.trace` 可以放外盘；采集脚本将小型摘要自动镜像到本机 `out/profile-summaries/`，不镜像巨型图状态和二进制。

只整理已有窗口：`resident_report.py --run SAVED_WINDOW_ROOT`，复用已有摘要，缺少 counter XML 时流式导出。只展示已有数据：

```sh
python3 "$PROFILE_SKILL/scripts/dashboard.py" \
  --profile "$NEW_PROFILE/summary.json" \
  --hardware "$NEW_COUNTERS/hardware-summary.json" --output out/profile-dashboard
```

页面包含阶段占比、可筛选的全部 kernel 与长尾、全部硬件指标及其覆盖秒数、验证和工作量。示例和最新结论见 [2026-10-30 resident 报告](references/2026-10-30-results.md)。

## 旧 DVB6/7 适配器

1. 检查 `df -h`、正在运行的 GPU workload，以及 `xcrun --find xctrace` / `gpucapture` / `gpudebug`。不要与其它 GPU benchmark 或训练并行，也不要停止别人的任务。
2. `hotspots` 读取完整运行已有计时，选择准备、匹配、RHS、rebuild 的热点 Run。
3. `prepare` 在独立目录编译带标签的 worker；原 shader、规则、调度不变，源码指纹必须匹配报告。
4. `record --mode counters` 抓热点附近的短窗口；一次能覆盖几个阶段就不重复采集。
5. `export` → `report.py` → `counters.py`，得到队列时间线与按 shader 区间加权的硬件计数器。
6. 关键 shader 再 `record --mode capture` 抓一个 Run，用 `capture_report.py` 获取寄存器、spill、指令类型和 limiter。已嵌入 profile 用 `--reuse`，不重新 profile。

```sh
python3 "$PROFILE_SKILL/scripts/profile.py" hotspots --run "$RUN"
python3 "$PROFILE_SKILL/scripts/profile.py" prepare \
  --run "$RUN" --source engine/metal/worker.swift --output out/profile-build
python3 "$PROFILE_SKILL/scripts/profile.py" record \
  --run "$RUN" --build out/profile-build --output out/profile-window \
  --mode counters --start-after "$START" --end-run "$END" \
  --seconds 3 --template 'Cell Metal Limiters' --waiting-threads
python3 "$PROFILE_SKILL/scripts/profile.py" export \
  --trace out/profile-window/profile.trace --output out/profile-xml
python3 "$PROFILE_SKILL/scripts/report.py" --exports out/profile-xml \
  --replay out/profile-window/replay.json --output out/profile-window/summary.json
python3 "$PROFILE_SKILL/scripts/counters.py" --exports out/profile-xml \
  --replay out/profile-window/replay.json --output out/profile-window/counters.json
python3 "$PROFILE_SKILL/scripts/profile.py" record \
  --run "$RUN" --build out/profile-build --output out/profile-capture \
  --mode capture --start-after "$START" --end-run "$((START + 1))"
python3 "$PROFILE_SKILL/scripts/capture_report.py" \
  --trace out/profile-capture/hotspot.gputrace --output out/profile-capture/analysis
```

`PROFILE_SKILL` 是本 skill 的绝对路径，`RUN` 是保存的运行目录，START 是跳过的完整 Run 数，END 是最后执行的 Run 序号。输出目录必须新建。复查已有 summary；对 `.gputrace` 用 `capture_report.py --reuse --output NEW`。不要重跑选最快结果。

先用 `xctrace list templates` 确认本机有 `Cell Metal Limiters`。其它机器可不传 `--template`，使用内置 GPU / Metal Application / Metal GPU Counters。若缺所需 counter，在 Instruments 中选择设备与 Performance Limiters counter set 并保存模板，再传名称。不要硬编码设备 registry ID 或把空 counter 表当作零。

## 证据与口径

- 保留输入、worker 指纹、Run 范围、工具版本、GPU 型号、counter 集。
- 逐 Run 核对 rows / logical matches / updated / epoch / unions；这是快速重放检查，**不是最终整图或原生语义判等**。
- GPU active 时间、无目标进程 GPU 工作的间隙、command buffer 数量，与 occupancy / ALU / 带宽分开列。
- CPU 等待需要 `--waiting-threads` 的 Time Profiler，不能只从 GPU 间隙推断 CPU 忙在哪里。
- GPU 队列 active 比例不等于 occupancy，也不等于算力或带宽利用率。
- shader timeline 是采样重建；长尾若落在未采样核心上，采样时长会远短于 encoder Duration。
- **gpudebug Cost % 不是 elapsed-time share**。按 Duration 找慢 encoder，再看 limiter；保留 sampled cores / total cores。
- 硬件计数器通常是设备级数据。关联目标 shader 区间不等于排除其它进程，不能把桌面渲染的 FP16/F32 活动算作 eggfuse 矩阵运算。
- 保留目标 shader 样本覆盖率和其它进程重叠率。没有目标 shader 样本时只报告设备窗口指标；几个微秒样本不能代表整个窗口。
- 一个 encoder 可能合并多个 kernel。保留整个组合，不能把耗时归给标签里的第一个 kernel；具体归因看 shader timeline 或单 Run replay。
- `indirect=true` 时标签中的 `threads=1` 是占位参数，不是实际派发宽度。

## 可靠性与资源

- 默认短窗口。完整长 trace 可能生成几十 GB 临时文件并耗大量内存；用完整运行计时选点覆盖各阶段。`--seconds 0` 是显式高开销选项。
- 用 Darwin `--notify-tracing-started` 通知，不猜英文日志或固定 sleep。时间参数用整数毫秒，不能传 `3.0s`。
- recording options 从 `--show-recording-options` 完整 JSON 修改，不能只提交一个字段。
- xctrace 错误、XML 不完整、目标 PID 无事件或 counter 表为空时保留失败，停止归因。SQLite issue 的数字 type 不是 severity；已验证的 `Data stream: Time Mapping` 是元数据头，未知 issue 需检查。
- trace、capture、XML、二进制、缓存只放 `out/`。只提交 skill 和小型文本结果。遗留 `instruments*.ktrace` / `xrgpu_aps_*` 仅在确认属于本次采集、进程退出且未打开后清理，禁止通配删除系统临时目录。
- gpudebug 只关闭自己创建的 session，不能 `--terminate all`。脚本自动清理自己的 session。
- `Profile data collected` 不保证逐 encoder 数据可用。`Unknown`、空 shader 列表、全部挂到整帧的占位区间均不能当有效 kernel 耗时。脚本保留 `partial.json/failure.json`；此时依靠独立 live timestamps 和 counters。`--execution serial` 是另一个诊断条件，不是挑更快的 benchmark。`--reuse --profile-index N` 可读取指定嵌入结果。
- 逐 kernel replay 不可用时，仍可用 `capture_report.py --inspect-dispatch commands/cbN/ceN/dispN ...` 读取捕获的网格、线程组和 pipeline。路径从 command tree 取得，不猜序号；网格宽度仍不等于有效负载或 occupancy。

详见 [工具语义](references/profiling-notes.md) 与 [实测结果](references/2026-10-26-results.md)。修改后运行 `python3 -m unittest discover -s "$PROFILE_SKILL/scripts" -p 'test_*.py'`；新增 worker/Xcode 版本还需真实短窗口和 capture/reuse 验证。

### Resident warm windows and frozen workers

For the resident ABI7 timing adapter, `--mode timings --runs N` profiles N logical
Runs from a physical checkpoint and verifies the complete suffix against the
same frozen executor. Use a short warm window when a single first dispatch could
be dominated by restored-buffer residency; retain the cold first-dispatch caveat.
Capture mode remains exactly one Run. Never treat split-command-buffer timing as
an end-to-end benchmark.

New checkpoints contain a checksum-bound worker image and materialized source.
The adapter uses these when present, so later workspace edits do not silently
change a historical replay. Older checkpoints without an image still require
the exact original worker. Do not bypass a worker mismatch. Instrumented
checkpoints remain diagnostic only and cannot be used as ordinary resume points.

### Encoder timestamp counters（快照模式）

`resident.py --mode encoder --runs N` uses the public Metal timestamp counter set
at encoder boundaries inside the original command buffers. It still separates
encoders and can perturb timing; it does not report occupancy or bandwidth.
On the inspected M3 Max, dispatch-boundary sampling is unsupported and
stage-boundary sampling is supported. A timestamp calibration matched resolved
counter deltas in nanoseconds against command-buffer GPU time. Do not divide
these resolved values by Metal 4's `queryTimestampFrequency` (24 MHz here): that
is a different raw timestamp representation. This mode retains exact suffix
state/history verification and frozen-worker provenance.

## 完整结果如何展示

同时给出：无插桩基线、诊断墙钟与扰动比例、累计 GPU command 时间、encoder 覆盖时间及未归属间隔、各阶段秒数与 p50/p95/p99/max、外层与私有提交/等待、真实工作计数、硬件计数器的样本数/覆盖秒数、验证和指纹。

CPU wait 包含 GPU 执行，不能和 GPU 时间相加；command 减 encoder 时间不是纯屏障耗时；issued encoder 不是有效 kernel 数。硬件计数器、静态指令和寄存器、动态耗时分别展示。绝不将诊断各阶段强行按比例缩放成无插桩基线的分解，也不生成虚假的“算术/访存/同步相加等于100%”图。

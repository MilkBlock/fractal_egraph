---
name: eggfuse-metal-profile
description: 用 Xcode xctrace、Metal GPU Counters 和 gpudebug 定位 eggfuse/egg_layout GPU 后端的 kernel 热点、低占用率、寄存器溢出、带宽与 CPU/GPU 等待。复用保存的真实工作负载，生成可复查的诊断；不将 profiler 重放时间当成端到端 benchmark。
---

# Eggfuse Metal profiling

先读已有结果。实现或输入没变时复用 trace、导出和嵌入的 GPU profile，不重跑原生 CPU 对照。需要新采集时使用本目录脚本，避免重新拼协议和 XML 统计器。

当前适配器覆盖 `experiments/device_backend` 的 GPU 自有存储后端，接受保存的 `program.json`、`report.json` 和对应 `worker.swift`，支持 DVB6/7。不要使用旧 CPU 提交式 assisted matcher 的报告，也不要把这个后端叫 pristine egglog。其它 worker 需补明确适配器，不能猜测协议。

新增 **resident ABI7** 适配器：`scripts/resident.py`，用于 `engine/metal/generic_cell.swift`。
它使用 `eggfuse replay` 的完整 GPU 快照，仅执行下一个 Run；不使用 DVB6/7 协议、不重放前缀。
先用新编译的 program 保存目标 Run 之前的 checkpoint，再采集：

```sh
python3 "$PROFILE_SKILL/scripts/resident.py" --repo "$REPO" \
  --program "$PROGRAM" --checkpoint "$CHECKPOINT" --build "$BUILD" \
  --output "$PROFILE_OUTPUT"
python3 "$PROFILE_SKILL/scripts/capture_report.py" \
  --trace "$PROFILE_OUTPUT/run.gputrace" --output "$PROFILE_OUTPUT/analysis"
```

`PROFILE_OUTPUT` 可以指向用户指定的移动硬盘。适配器把子进程 TMPDIR 也放在那里，但不保证
所有 Apple 系统缓存随之迁移。它核对 capture 与无 capture 重放的完整状态；有已有单 Run
结果时传 `--reference result.json` 复用。capture 临时拆分 encoder 以分辨各阶段，采集耗时不可
当作生产加速比。快照要求同一程序、worker 和设备；切换实现后不能冒充旧状态仍可安全恢复。
Xcode 27.1 收集 profile 后需要显式 embed/load，才有 encoder 数据；空列表必须报错。
新适配器当前支持单 Run GPU capture；xctrace counters/CPU-wait 窗口仍仅由旧适配器提供。
真实 Pass-3 巨型 shader 的 Apple 重放编译可能超过默认 300 秒。发生时先确认编译与推理的
区别，保留失败，不自动反复采集；确需延长时显式传 `capture_report.py --timeout-seconds N`。

## 快速路径

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
- 一个 encoder 可能合并多个 kernel。保留整个组合，不能把耗时归给标签里的第一个 kernel；具体归因看 shader timeline 或单 Run replay。
- `indirect=true` 时标签中的 `threads=1` 是占位参数，不是实际派发宽度。

## 可靠性与资源

- 默认短窗口。完整长 trace 可能生成几十 GB 临时文件并耗大量内存；用完整运行计时选点覆盖各阶段。`--seconds 0` 是显式高开销选项。
- 用 Darwin `--notify-tracing-started` 通知，不猜英文日志或固定 sleep。时间参数用整数毫秒，不能传 `3.0s`。
- recording options 从 `--show-recording-options` 完整 JSON 修改，不能只提交一个字段。
- xctrace 错误、XML 不完整、目标 PID 无事件或 counter 表为空时保留失败，停止归因。SQLite issue 的数字 type 不是 severity；已验证的 `Data stream: Time Mapping` 是元数据头，未知 issue 需检查。
- trace、capture、XML、二进制、缓存只放 `out/`。只提交 skill 和小型文本结果。遗留 `instruments*.ktrace` / `xrgpu_aps_*` 仅在确认属于本次采集、进程退出且未打开后清理，禁止通配删除系统临时目录。
- gpudebug 只关闭自己创建的 session，不能 `--terminate all`。脚本自动清理自己的 session。

详见 [工具语义](references/profiling-notes.md) 与 [实测结果](references/2026-10-26-results.md)。修改后运行 `python3 -m unittest discover -s "$PROFILE_SKILL/scripts" -p 'test_*.py'`；新增 worker/Xcode 版本还需真实短窗口和 capture/reuse 验证。

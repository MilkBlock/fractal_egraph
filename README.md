# fractal_egraph / egg_layout

从 egglog 的实际执行历史构建 Coarse / Smooth layers，对注册的 coarse 入口执行原生 ripen，并将相同的 saturated rule composition 归入共享族。

当前主线是 Rust 数据结构，不再使用 Tier1 `.egg`、Use 模板搜索、启发式分割、自动候选预算或旧 Babble/Tier2 实验 API。旧实现可从 Git 历史查阅。

## 运行

```sh
cargo build --release --offline
# 有限、可复现的再入观察示例
cargo run --release --offline -- analyze \
  --source tests/fixtures/core_flow/reentry.egg --output out/reentry-demo
# math：--rounds 只覆盖 tier0 的简单 run 次数，不限制 ripen
cargo run --release --offline -- analyze \
  --source egglog/tests/math-microbenchmark.egg --rounds 6 --output out/math-new
```

输出目录必须不存在。省略 `--rounds` 时遵循源程序 schedule。Ripen 没有自动轮数、时间或工作量上限；未达到不动点的任务会持续执行，可用 Ctrl-C 取消。一个任务的一次完整 ruleset sweep 是调度粒度，不是终止预算；单次 sweep 本身也可能很昂贵。

默认不写完整历史。需要重新分析时：

```sh
cargo run --release --offline -- analyze --save-history \
  --source tests/fixtures/core_flow/reentry.egg --output out/history-demo
cargo run --release --offline -- analyze \
  --replay-history out/history-demo/history.json --output out/replayed-demo
```

在线采集与离线历史重放共用分析流程。旧 `--build offline`、`--reuse-tier0` 和实验 CLI 已移除；`--recapture-tier0` 仅保留兼容拼写。

## 查看每轮行为

```sh
python3 tools/egglog_debugger/server.py --port 8080
```

打开本地页面，在 layer 面板载入输出目录。`rounds/manifest.json` 索引每轮 JSON / DOT；共享族视图提供来源组合、Coarse/Smooth layers、原生 egraph 合并前后 body 和 Top100 去重覆盖统计。运行中的 ripen 会继续推送进度，停止按钮取消后台进程。详见 [调试页面](tools/egglog_debugger/README.md)。

主要结果：

- `analysis.json`、`layers.json`：已完成分析。
- `saturated-rule-composition/queue.json`：最新完整队列、CS 组合、weak dominate 和无法判定原因。
- `saturated-rule-composition/cells/`：每个入口的 `.egg`、ripen 状态和原生 DOT/SVG。
- `catalog/catalog.json`：完成且可导出的共享族；`catalog/states/` 保存精确状态。

取消后保留最近完整检查点，但目前不能从检查点恢复原生引擎继续运行。未完成的任务不会被记为 saturated。

## 核心流程与边界

1. `native_analyze.rs` 收集规则应用、读依赖、已提交写入与 union 证据。逻辑 match 不等于已提交 mutation。
2. `coarse_smooth.rs` 按实际依赖建立 layers；smooth/coarse 是相对于组合边界的分类。
3. `native_cs.rs` 由实际 consumer 连接登记 CSCS / CCSS 等组合。不会遍历任意 layer 对；无交叉支持保留 consumer 的独立 coarse 入口与联合入口。
4. `native_ripen_entry.rs` 将入口及条件、binding、外部注入义务转换为 symbolic entry；不支持的时序/操作明确拒绝，不伪装成已完成。
5. `native_ripen.rs` 为不同入口保留独立原生引擎。流水线公平轮转，每轮推进所有 live 任务；相同入口共享执行。
6. `saturated_rule_composition.rs` 在规则环境、接口、类型、字面量、可见性等条件下精确判等。没有搜索预算，最坏情况仍可能昂贵。
7. `native_weak_dominate.rs` 在原生状态中查询已登记的 coarse fact，通过必要 operator 索引筛选可匹配状态，保留 binding 与新增 effect 观察。查询由 `egglog/src/fact_query.rs` 编译为原生 Join，通过回调收集 binding，不创建临时 relation。每个引擎/入口保存一个当前查询与一次初始匹配结果；后续检查无需克隆引擎或重新编译。初始结果仍按当前 union 代表元校正。每个 ripen session 的初始快照仍保留，缓存也随已登记查询数量增长。

**Weak dominate 不等于完整闭包等价。** 新 binding、已提交新 effect 及同一共享族只产生再入观察；目前没有证明该 effect 导致具体再入，也没有归纳证明无限或耗散型 FractalRule。weak 匹配不会自动生成任意组合。尚未将共享状态替换进 tier0 matcher，因此覆盖统计不是实际 tier0 内存节省或加速证明。

当前捕获/导出仍有 datatype、函数更新、隐藏行及外部注入时序的支持边界，具体失败见队列 reason。没有预算不意味着恒定内存或自动终止；无限规则会持续增长。

## 验证

```sh
cargo test --release --offline
python3 tools/egglog_debugger/test_stream_errors.py
node tools/egglog_debugger/test_ripen_coverage.mjs
```

`tests/core_flow.rs` 验证 270 个观察入口全部登记、等价入口共享原生执行、有限再入，以及有限/无限任务共存时的进度与显式取消。测试测量实际 egglog runtime；取消期限仅属于测试进程，不是推理算法预算。

`egglog-baseline` 保留原版导入基准。用户尚未提交的旧实验文件可能仍在工作区，但不再注册为构建目标。

### Weak 查询复用对照

```sh
cargo test --release --offline --test native_fact_query benchmark -- --ignored --nocapture
```

本地一次 release 测量：200 个匹配 binding，重复 60 次，旧克隆+解析/编译路径 23.34 ms，缓存原生 Join 1.45 ms（约 16 倍）。这是原生查询层的热查询微基准，缓存编译在计时外；不代表整个 ripen / math 流水线加速。回归覆盖 union、插入、函数更新、subsume、类型/字面量/别名约束，以及不执行用户规则。

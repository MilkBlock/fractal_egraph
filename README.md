# fractal_egraph / egg_layout

从 egglog 的实际执行历史构建 Coarse / Smooth layers，对注册的 coarse 入口执行原生 ripen，并将相同的 saturated rule composition 归入共享族。

当前主线是 Rust 数据结构，不再使用 Tier1 `.egg`、Use 模板搜索、启发式分割、自动候选预算或旧 Babble/Tier2 实验 API。旧实现可从 Git 历史查阅。

Reuse Graph 的第一阶段已提供 `RipSatEgraph` 共享主体、封装/实例身份、graft 连接度和复用/破坏/补充计数，见 [数据结构与边界](docs/reuse-graph.md)。支持从新生成的原生 ripen 产物离线构建、保存和校验共享库；可用 `analyze --reuse-library FILE` 在线补充已完成的 ripen 结果，`reuse-query` 验证只读 Join 片段复用；`reuse-run` 可对正向有限程序做 LHS 摘要替换并对照原生执行；`reuse-cycle` 贯通分轮反馈验证。原生可变表尚未虚拟化，默认 matcher 保持原样。

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

输出目录必须不存在。省略 `--rounds` 时遵循源程序 schedule。Ripen 默认使用 CLI / 网页共享的 50 ms、200K facts 上限；可显式指定 `--ripen-observe-ms N` 或 `--ripen-observe-facts N`，在原生匹配入队和 Join 批次边界停止推进，保存并挂起未完成观察。可用 Ctrl-C 取消。一个 sweep 默认最多推进 4 轮，每轮让各 ruleset 各执行一步；遇到不动点或显式上限提前结束。可用 `--ripen-sweep-steps N` 调整。单次原生 step 仍可能很昂贵。

`analyze` 默认尝试复用已完成的父 RipSat 快照，再执行精确的 graft 后缀。目前仅接纳正向构造/关系规则、稳定的边界 token 与完整历史前缀；`set`、`subsume`、primitive 条件等环境会回退原路径。同批次的合法子组合会等待父状态，父状态未完成或被拒绝时回退重建。统计在 `analysis.json` 的 `saturated_rule_composition.parent_snapshot_reuse`，每个 job 的 `initialization` 标明实际路径。设置 `EGG_LAYOUT_RIPEN_PARENT_REUSE=0` 可作关闭对照。当前只省初始化回放，完整冷准备验证仍保留，也没有终点状态缓存。详见 [接入验证](experiments/ripsat_graft/README.md#default-analyze-integration)。

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

完整 math-microbenchmark 的 bake 决策评估见 [实验报告](experiments/math_ripen_report/README.md)：包含 11 轮原生/trace 对照、未完成的完整流水线、初始结构与饱和结构判等 ablation，以及实际覆盖分母。

可选 CC 放开合并实验：`cc-merge --rules FILE --seeds FILE --output DIR --generations N`。按代穷举饱和族的配对与 binding，支持新族递归接续，输出增长曲线数据；不改变默认分析流程。配置与支持范围见 [CC 实验说明](experiments/cc_merge_probe/README.md)。

## Non-trivial reentry 桶

现在会把结构再入观察归组，并维护多个已观察的最小 starting conditions：

- 原来的精确 weak 查询不变；另做擦除字面值的结构查询，避免 length 衰减让相同结构完全无法进入桶。
- 需要新增 effect 和相对于初始图新增的结构 binding；这只是再入嫌疑，不是因果归纳证明。
- 第一版“成分类似”定义为新增节点的 operator 集合相同、union 标记相同、规则环境相同。数量作为证据保留，不使用阈值、打分或 Top-K。
- 在桶内使用 egglog 原生 Join 检查 coarse fact 的结构 cover：保留类型、参数位置、重复变量的相等约束和命名接口；不同变量不被额外假定为不相等。这里只比较擦除值后的条件，不是完整规则行为判等。
- 已观察的小条件替代大条件进入最小集合；互不 cover 的条件都保留，大条件及原始记录也保留。双向 cover 表示相同抽象条件，不表示相同 FractalRule。
- 字面值仅从结构查询和抽象条件中擦除，原始条件仍保存。不改变实际 rule 执行，不用抽象结果替换 tier0，也不分类 infi / dissipative。

包含 primitive 条件的图可导出 `trigger-state.json` 用于观察；它不会因此获得原本缺失的饱和共享证明。当前只处理已登记 CS 的观察，不搜索未出现的更小条件，不枚举循环体。显式设置观察阈值时，增长中的整个 CS 可直接入桶，不要求自己的 C 再次出现。

`queue.json` / 最终分析里的 `reentry_buckets` 包含成分桶、cover 映射、最小条件集合、观察记录和原始条件；`structural_reentry` 保留擦除值后的查询证据。网页饱和组合面板的 weak 信息下方可展开 **non-trivial reentry**，点击条件查看证据。查询绑定数不是实际 rule apply 次数。

快速例子（真实有限递减，不会被宣布为 infi）：

```sh
cargo run --release --offline -- analyze \
  --source tests/fixtures/core_flow/length_reentry.egg --output out/reentry-length-new
```

回归覆盖多个互不可比的最小条件、大条件保留、别名约束、值擦除但保留原文、成分分组与实际有限长度再入。核心实现：`src/reentry_bucket.rs`。

### 增长先入桶，随后查已有 C

```sh
cargo run --release --offline -- analyze \
  --source experiments/reentry_forms/dabc.egg --output out/dabc-observed \
  --ripen-observe-facts 11
```

也可用 `--ripen-observe-ms N` 限定每个 cell 的累计 ripen 推进时间（不含准备和其他 cell 等待时间）。调试网页可在启动服务时设置 `EGG_LAYOUT_RIPEN_OBSERVE_MS` / `EGG_LAYOUT_RIPEN_OBSERVE_FACTS`。两个限制默认分别为 50 ms 和 200K facts，任一个达到即记录。原生 Join 检查时间，匹配入队前按动作指令数保守预留待写额度；额度耗尽记为 pending_fact_reservations，可能早于实际 facts 上限。已入队动作与必要 rebuild 完成后返回 unfinished，因此不是硬实时或整个进程的内存上限。

未完成任务标为 `ObservedUnfinished`，整体分析标为 `observed_unfinished`。若事实数比起始图增加，整个 CS 以 `growth_observation` 入桶，`starting_condition_resolved` 保持 false。未增长但超过时间的任务只保留未完成记录，不硬称增长。已有共享任务跟随 owner 的未完成状态，不会永久停在 WaitingEquivalent。

保存 `unfinished-state.json`（仅观察图，不是饱和证明）。随后遍历桶中已有 C，在未完成图上查询，结果放入 `bucket_trigger_matches`；分别标记“包含已有 C”和“出现新的 binding”。初始 C 一直存在不再阻止入桶，也不会被当作已证明的循环。

这些已知 C 的命中与桶内直接的 coarse fact cover 分开保存，不凭一次命中宣布两个 FractalRule 等价。验证说明见 [DABC 与多叉测试](experiments/reentry_forms/README.md)。

每个 cell 的 `ripen.json` 中 `sweep.steps` 记录本批实际推进轮数，`sweep.max_steps` 记录配置值。`EGG_LAYOUT_RIPEN_SWEEP_STEPS` 可配置分析/调试流程，默认 4；`--ripen-sweep-steps` 可覆盖它。

默认不发送逐 application 的 DOT / Typst / binding 展示日志；FractalRule、覆盖统计和图仍保留。需要完整逐条展示时设置 `EGG_LAYOUT_FULL_TRACE=1`（网页服务也读取此环境变量）。`--save-history` 仍显式保存可重放分析历史。tier0 的内部依赖事件仍在提交后被分析并排空，保留用于判定 composition 的 provenance。

SVG 排版在分析结束后统一并行执行，不再阻塞每轮 ripen。默认独立线程池并发 2 个 Graphviz 进程，可设置 `EGG_LAYOUT_SVG_THREADS=4` 调整；`EGG_LAYOUT_EAGER_SVG=0` 仍关闭生成。同一路径只渲染最终 DOT，共享实例复制已完成的 SVG。CLI 等待全部图表完成后退出，网页显示单独的渲染阶段。单张复杂图仍可能排版很慢，并行不会消除这一成本。

网页运行按钮旁的“生成 DOT / SVG”复选框可关闭本次运行的图表生成，保留分析和统计 JSON。本地服务逐请求传递此配置；CLI 可使用 `EGG_LAYOUT_GRAPHS=0 cargo run --release --bin egg_layout -- analyze --source FILE --output DIR`。此开关比只关闭 SVG 更彻底，也跳过 DOT 和 layer 图构造。

无图表 CLI 分析只保存最终分析和判等目录等数据，不再构造/保存逐轮展示快照；网页仍推送实时统计。局部 ripen 默认只保留真实提交的 effect 摘要，使用 `EGG_LAYOUT_RIPEN_HISTORY=1` 可恢复局部 binding 和 Coarse/Smooth 历史；tier0 的完整历史分析不变。

无图表分析使用 4 个独立任务线程，可通过 `EGG_LAYOUT_ANALYSIS_THREADS` 设置（兼容旧名 `EGG_LAYOUT_QUERY_THREADS`）。准备好的 AST、查询计划、未变化状态的查询结果会复用；每个任务拥有独立 egraph 和提交通知队列。

weak-dominate 默认返回“存在匹配 / 存在新匹配”的精确见证，`bindings_complete=false` 表示 binding 列表不穷举。发现新匹配即可停止只读 Join；无新匹配时仍完整执行以证明不存在。`EGG_LAYOUT_FULL_TRACE=1` 恢复完整 binding 枚举。它与 `EGG_LAYOUT_RIPEN_HISTORY=1` 分别控制查询证据和局部执行历史。

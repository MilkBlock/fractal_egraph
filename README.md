# egg_layout

从 rule apply 历史分析组合，用 FractalComb 表示稳定重复，用 Reduce 提取终点表达式，并生成 fractal 可视化。

## egglog-demo 交互调试

`python3 tools/egglog_debugger/server.py` 启动本地调试页面：点击 `.egg` 规则行查看 Typst / DOT，
使用“运行并识别”逐轮接收真实 runtime 的 Compose / Fractal 日志，点击日志重现公式。
支持日志导出、导入和源码恢复。参见 [调试桥说明](tools/egglog_debugger/README.md)。

## 使用：单个 Rust 进程

```sh
cargo run --release -- analyze --recapture-tier0 \
  --source egglog/tests/math-microbenchmark.egg --output out/native-math
# 不传 --rounds 就遵循文件的 run；显式传入才覆盖单个简单 run。
cargo run --release -- analyze --recapture-tier0 \
  --source egglog/tests/math-microbenchmark.egg --rounds 6 --output out/native-six
cargo run --release -- analyze --reuse-tier0 --output out/native-six
```

新入口不启动 Python、研究程序或管道。采集事件、构建 tier-1/tier-2、检查 binding 和生成页面都在同一进程中完成。
数据直接使用原生事件、Rust 结构体和 egglog AST/Value；Tier1 的 coarse/smooth layers、binding 与 effect 支持检查由 Rust 构建；现有 Tier2 通过只含声明和已验证数据的接口继续使用 egglog。默认保存最终 `layers.json`、`analysis.json`、`run.json`、`fractal.html`、`index.html`，以及 `rounds/` 中每轮的 layer/fractal/coverage DOT、完整渲染快照 JSON（在 tools 调试网页载入）；不生成完整中间 trace。
页面为输出目录下的 `fractal.html`；输出目录必须不存在。复用模式只重新渲染已完成结果，不重新执行推理。
不指定复用目录时，使用仓库已有的固定视图。旧管道缓存的完整视图仍可复用，但不会启动旧流水线。

当前输入要求是自包含的单个 datatype（名字任意：`Math`、`Expr` 都一样分析）；不是任意 `.egg` 的通用导入器。
原生 TraceSession 在每个简单 run 轮次结束后移交并清空原始事件，随后在线更新 tier-1。跨轮保留精简的 producer 行证书与 union 等价边；**在线构建不等于恒定内存**，单轮事件和分析图仍可能很大。

### 在线、离线与历史重放

```sh
# 在线（默认）：每轮结束后增量构建同一个 tier-1，再执行下一轮 tier-0。
# 可选保存 history.json；不指定 --save-history 就不写历史。
cargo run --release -- analyze --recapture-tier0 --build online --save-history \
  --source egglog/tests/math-microbenchmark.egg --rounds 6 --output out/online

# 离线：先运行完 tier-0，再统一构建 tier-1/tier-2。
cargo run --release -- analyze --recapture-tier0 --build offline --save-history \
  --source egglog/tests/math-microbenchmark.egg --rounds 6 --output out/offline

# 真正重新分析：从历史重建 tier-1/tier-2、重新提取和渲染，不执行 tier-0。
cargo run --release -- analyze --replay-history out/online/history.json --output out/replayed
```

三条路径共享 collector/builder。`--reuse-tier0` 仍仅重画最终视图，不能替代 `--replay-history`。
复杂 schedule 保持原语义，当前在线导入边界仅拆开简单 repeat(run)；复杂 schedule 的数据在采集结束时导入。

`history.json` v1 保存规范化原规则、AST 位置映射、类型/token 字典，以及有效 apply 的
parents、ports、inputs/outputs、binding、读依赖、写事实和 union effect。它是**已解析的应用历史**，
不是全量内核 trace：不能用来重新判断被排除的 match 或重新选择原始写入 provenance。
重放不需要原 `.egg` 文件，使用当前 tier-1/tier-2 分析规则；跨版本 schema 不兼容会拒绝。
历史在 tier-0 采集完成后、最终分析前写入，缓冲序列化并原子发布；中断残留的 `.partial` 不是有效历史。
这不是逐轮断点恢复；默认不开历史以避免磁盘负担。在线和离线模式都逐轮释放原始事件，离线模式仅推迟 tier-1 构建。`run.json`/`analysis.json` 记录原始批次峰值事件数、消费批数和剩余事件数。

实际 Math 6 轮对照检查在线/离线历史的 binding 和 effect 完全一致，重放后
combined rule 文本、source steps、relative routes 与视图统计一致；native eclass 分配编号不要求一致。

新的结构及旧术语迁移见 [coarse/smooth layers](docs/coarse-smooth-layers.md)。旧链式 Tier1 解释器仅保留在 `research/legacy_tier1.egg`，供历史实验和回归对照，主路径不加载它。

## 精简的实现阅读顺序

| 文件 | 职责 |
|---|---|
| [coarse/smooth layers](src/coarse_smooth.rs) | Rust 组合定义去重、实例、binding/effect 验证、见证驱动的层接口 |
| [Layer 模板与有限递归](src/layer_patterns.rs) | 接口切片、返回 binding、结构覆盖与预算状态 |
| [逐轮图](src/native_layer_view.rs) | 真实轮次快照、DOT 与 tools 网页数据 |
| [Tier2 数据接口](src/layer_bridge.rs) | 仅类型/关系声明，没有 Tier1 推理规则 |
| [tier-2 IR](rules/tier2.egg) | 稳定扩展、重复观察、坐标变换 |
| [FractalComb](rules/higher.egg) | 已有组合链 → 次数参数 k |
| [Reduce](rules/reduce.egg) | 显式归约及解析表达式成本 |
| [主入口](src/main.rs) | 命令选择 |
| [单进程分析](src/native_analyze.rs) | 原生事件 → tier-1 → tier-2 → 结果 |
| [历史重放](src/native_history.rs) | 可选 JSON 保存、位置恢复与引用校验 |
| [组合显示](src/native_lower.rs) | 选中 DAG 的 symbolic lowering，保留中间 effect |
| [原生适配接口](src/pipeline.rs) | 统一原生执行、查询、导出；递推 fixture 明确标注 |
| [页面模板](experiments/tier2/fractal_view_template.html) | 交互界面；数据由 Rust 生成 |

Tier0 规则和 Tier2 数学规则仍由原生 egglog 执行，Tier1 已由 Rust layers 接替。当前主线主要阅读 `native_analyze.rs`、`native_lower.rs` 与 `rules/`。
`pipeline.rs` 保留独立诊断命令，`visual_rule.rs` 提供共享 AST 规范化。历史研究代码位于独立 research 工程。
Rayon 仅用于给元图分析分配一个工作线程池；tier-0 保留正常执行池，所有线程仍属于同一进程。
历史 Python 流水线保留供对照，已不在默认 analyze/view 路径中。

更详细的实验范围、假设与结果见 [tier-2 说明](experiments/tier2/README.md)。
旧 Zobrist、babble、prefix、slotted 等路线的导航见 [research](research/README.md)。

旧命令改为从仓库根目录执行：

```sh
cargo run --manifest-path research/Cargo.toml --bin rule_combine
cargo check --manifest-path research/Cargo.toml --all-targets
```

## FractalRule Bake 与固定库复用

```sh
cargo run --release -- bake experiments/bake/manifest.json out/my-bake
cargo run --release -- bake-use out/my-bake/library.egg experiments/bake/heldout-binary.egg out/my-use
cargo run --release -- bake-eval out/my-bake/library.egg fractal_0001 11 --depth 20 out/frontier.json
```

Bake 离线合并多个小样例；固定库模式不发现新模板、不重建 tier1/tier2，但仍执行原 tier0。
通过结构检查的计数/完整 radix 前沿，可以单独使用数组 DSL 做无 tier0 展开的值查询。
这不等于跳过任意程序的中间 effect。输入、证明边界与对照实验见 [Bake 说明](experiments/bake/README.md)。

## Tier2 first-class 数组

逻辑数组 DSL 在 `rules/arrays.egg`，支持参数化长度、Tabulate、Map/Zip、Slice、
有序 Fold、Sum/Max、分块和符号索引。默认不展开元素；有限具体求值需要显式预算。
数组可作为 binding 参数传递。现有 FractalComb 通过显式数组归约契约连接到 EndpointView。

```sh
cargo run --release -- arrays experiments/arrays/basic.egg out/arrays.json
cargo test --test arrays
```

这是原生 tier2 DSL 的执行和提取；上面的 Bake 可生成部分已检查的数组摘要，GPU 内存调度与 FlashAttention 搜索尚未实现。
设计与 eggcc DSL 的对应关系、测试边界见 [数组说明](experiments/arrays/README.md)。

## 验证和边界

```sh
cargo test --test native_single_process --test tier2_native --test tier2_reduce --test pipeline_cli
cargo test --release --test native_single_process --test native_history -- --include-ignored
```

回归基准包括 13 个有限 FractalComb、4 条 fractal 轨道、Reduce 结果与反例。
有限重复不等于任意 k 的闭合证明；视图隐藏非 fractal 区域，不声称全图无损压缩。

保留 `egglog-baseline` 和完整 Git 历史：

```sh
git diff egglog-baseline..HEAD -- egglog/
git log --oneline --reverse egglog-baseline..HEAD
```

## FractalComb 规范化视图

`FractalComb(Depth(k), extension, start_ctx, initial_binding)` 与 SmoothComb、CoarseComb 同属 Comb。
其计数从有 witness 的 SmoothComb 开始；coarse 注入及启动历史留在 start_ctx。
`analysis.json` 的 `fractal_views` 保存选中的组合、段内 fact/effect 历史和中间输出引用。
页面的接口证据可查看这些数据，tier-1 表示显示选中的 PackedComb 表达式。

FractalComb 后可以接普通规则，再启动另一段。实例 fact 按“段末 occurrence、段内位置、端口”寻址，
不会因模板共享而混淆来源。当前是保留原始实例的经过校验的规范化视图，**尚未用它替换在线导入、删除中间节点或证明任意次数的稳定性**。
启动位置是观测到的有限稳定接口边界，不是自动证明的最小 trigger state。

相关规则见 [fractal_views.egg](rules/fractal_views.egg)，反例与跨段测试见 [fractal_comb.rs](tests/fractal_comb.rs)。

Math 6 轮验证了全部 1529 个实例的规范化视图，其中有 8 个选中的 FractalComb 段视图、24 条段末继续组合关系、116 个父依赖的段内地址。
2366 对原始 SupportsUse 不变。原始 Comb 模板 1389 个，保留原图并加入视图后共 1435 个；这些数字不代表已实现存储压缩。

## 参数化依赖候选实验

用 `EGG_LAYOUT_DISCOVER_GROWTH=1` 开启主干/旁支分析，默认关闭。它尝试按真实父依赖识别
`(d-1,j) → (d,j)` 与每层新增位置 `j=d`，并用后续层检查 binding/effect 模板。
结果和可点击的位置图分别写入 `dependency_growth.json`、`dependency_growth.html`。
详见 [增长旁支实验](experiments/dependency_growth/README.md)。这是有限候选分析，不是无限递归证明或可执行规则反馈。

## 共享模板与排序

`EGG_LAYOUT_TEMPLATE_CATALOG=1` 开启共享模板目录（兼容 `EGG_LAYOUT_DISCOVER_RECURSION=1`）。
目录统一列出线性及多出口 FractalComb，共享步骤定义、展开范围与事件证据；优先遵守有证书的观测 dominance，再按去重后的新增覆盖排序。
它不把覆盖率当作语义等价，也不对不同模板做未经证明的 union。
使用说明、正反例与结果见 [模板目录实验](experiments/recursive_patterns/README.md)。

结构 dominance 使用允许额外节点/边的有限 DAG 嵌入，不要求入口或事件 ID 相同。
检查器返回节点/接口映射；超预算明确返回 unknown。使用方式和完整性边界见 [DAG 嵌入实验](experiments/dag_embedding/README.md)。

## Demo 注释与公式文字编辑

`egglog-demo` 的 `examples.py` 在生成示例 bundle 时会加入插件支持的
`egg-viz/v1` 注释：datatype/constructor 的 Typst 模板、优先级，以及显式
`rule` 的 binding/position 显示名称。注释只存在于生成的文本 bundle，不改变
egglog 程序的可执行 token。

调试页面中，Typst 公式里的高亮文字可以点击编辑。保存会更新对应的
`dsl_type` 或规则 `labels` 注释，立即重渲染；编辑器支持撤销/重做，并可下载
当前 `.egg` 源码。历史日志的公式保持只读，避免修改回放快照。

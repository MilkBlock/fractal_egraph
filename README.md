# egg_layout

从 rule apply 历史分析组合，用 HigherRule 表示稳定重复，用 Reduce 提取终点表达式，并生成 fractal 可视化。

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
数据直接使用原生事件、Rust 结构体和 egglog AST/Value；只保存最终 `analysis.json`、`run.json`、`fractal.html`、`index.html`。
页面为输出目录下的 `fractal.html`；输出目录必须不存在。复用模式只重新渲染已完成结果，不重新执行推理。
不指定复用目录时，使用仓库已有的固定视图。旧管道缓存的完整视图仍可复用，但不会启动旧流水线。

当前输入要求是自包含的单个 Math datatype；不是任意 `.egg` 的通用导入器。
底层原生 TraceSession 仍保留事件到转换完成，所以**单进程不等于在线或恒定内存**。

## 精简的实现阅读顺序

| 文件 | 职责 |
|---|---|
| [tier-1 IR](experiments/tier1_effects/tier1_rule_comb_ir.egg) | Comb、relative binding、实例及 effect |
| [tier-2 IR](rules/tier2.egg) | 稳定扩展、重复观察、坐标变换 |
| [HigherRule](rules/higher.egg) | 已有组合链 → 次数参数 k |
| [Reduce](rules/reduce.egg) | 显式归约及解析表达式成本 |
| [主入口](src/main.rs) | 命令选择 |
| [单进程分析](src/native_analyze.rs) | 原生事件 → tier-1 → tier-2 → 结果 |
| [组合显示](src/native_lower.rs) | 选中 DAG 的 symbolic lowering，保留中间 effect |
| [原生适配接口](src/pipeline.rs) | 统一原生执行、查询、导出；递推 fixture 明确标注 |
| [页面模板](experiments/tier2/fractal_view_template.html) | 交互界面；数据由 Rust 生成 |

规则语义仍由原生 egglog 执行。当前主线主要阅读 `native_analyze.rs`、`native_lower.rs` 与 `rules/`。
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

## 验证和边界

```sh
cargo test --test native_single_process --test tier2_native --test tier2_reduce --test pipeline_cli
cargo test --release --test native_single_process -- --include-ignored
```

回归基准包括 13 个有限 HigherRule、4 条 fractal 轨道、Reduce 结果与反例。
有限重复不等于任意 k 的闭合证明；视图隐藏非 fractal 区域，不声称全图无损压缩。

保留 `egglog-baseline` 和完整 Git 历史：

```sh
git diff egglog-baseline..HEAD -- egglog/
git log --oneline --reverse egglog-baseline..HEAD
```

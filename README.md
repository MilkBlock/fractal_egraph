# egg_layout

从 rule apply 历史分析组合，用 HigherRule 表示稳定重复，用 Reduce 提取终点表达式，并生成 fractal 可视化。

## 使用

```sh
cargo run -- analyze   # 从固定 tier-1 快照重现当前 tier-2 流程，并运行检查
cargo run -- view      # 从已有结果重新生成 fractal 页面
cargo run -- --help
```

页面位于 `experiments/tier2/fractal.html`。`analyze` **不是重新采集任意 .egg 的 tier-0 trace**；
当前仍使用已保存的 Math tier-1 数据，递推测试另有专用适配器。

## 精简的实现阅读顺序

| 文件 | 职责 |
|---|---|
| [tier-1 IR](experiments/tier1_effects/tier1_rule_comb_ir.egg) | Comb、relative binding、实例及 effect |
| [tier-2 IR](rules/tier2.egg) | 稳定扩展、重复观察、坐标变换 |
| [HigherRule](rules/higher.egg) | 已有组合链 → 次数参数 k |
| [Reduce](rules/reduce.egg) | 显式归约及解析表达式成本 |
| [主入口](src/main.rs) | 命令选择 |
| [原生适配接口](src/pipeline.rs) | 统一原生执行、查询、导出；递推 fixture 明确标注 |
| [流程](experiments/tier2/run.py) | 单个程序驱动的重现实验 |
| [Fractal 视图](experiments/tier2/fractal_view.py) | 实际实例依赖 → 最大轨道与点击数据 |

规则语义仍由原生 egglog 执行。Rust 运行器只保留一份实现；
旧程序位于独立的 `research` 工程，不参与默认构建。原有兼容入口在那里保留。

默认 Rust 工程只有 `main.rs`、`lib.rs`、`pipeline.rs`、`visual_rule.rs` 四个源文件参与编译，
只依赖 egglog 和 serde_json。历史研究代码已迁入独立 Cargo 工程。
有未提交修改的 `src/pattern_store.rs`、`src/tier1_effects.rs` 和三个旧 binary 暂留原处，
仅由 research 引用；不属于默认工程。

完整实验仍有 Python 数据准备与可视化脚本，这一层尚未改写为 Rust；
因此这里的“四个文件”指原生外挂层，不宣称整个端到端研究流程已只有四个文件。
更详细的实验范围、假设与结果见 [tier-2 说明](experiments/tier2/README.md)。
旧 Zobrist、babble、prefix、slotted 等路线的导航见 [research](research/README.md)。

旧命令改为从仓库根目录执行：

```sh
cargo run --manifest-path research/Cargo.toml --bin rule_combine
cargo check --manifest-path research/Cargo.toml --all-targets
```

## 验证和边界

```sh
cargo test --test tier2_native --test tier2_reduce --test pipeline_cli
```

回归基准包括 13 个有限 HigherRule、4 条 fractal 轨道、Reduce 结果与反例。
有限重复不等于任意 k 的闭合证明；视图隐藏非 fractal 区域，不声称全图无损压缩。

保留 `egglog-baseline` 和完整 Git 历史：

```sh
git diff egglog-baseline..HEAD -- egglog/
git log --oneline --reverse egglog-baseline..HEAD
```

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
| [tier-2 IR](experiments/tier2/ir.egg) | 稳定扩展、重复观察、坐标变换 |
| [HigherRule](experiments/tier2/higher_ir.egg) | 已有组合链 → 次数参数 k |
| [Reduce](experiments/tier2/reduce_ir.egg) | 显式归约及解析表达式成本 |
| [主入口](src/main.rs) | 命令选择 |
| [原生适配接口](src/pipeline.rs) | 统一原生执行、查询、导出；递推 fixture 明确标注 |
| [流程](experiments/tier2/run.py) | 单个程序驱动的重现实验 |
| [Fractal 视图](experiments/tier2/fractal_view.py) | 实际实例依赖 → 最大轨道与点击数据 |

规则语义仍由原生 egglog 执行。Rust 运行器只保留一份实现；
旧的 `tier2_run`、`tier2_higher`、`tier2_reduce`、`tier2_observations`、`tier2_fixture`、
`tier1_source_schema` 程序名是兼容入口，不再各自维护逻辑。

本轮是主流程收拢：Python 转换脚本、固定快照及历史实验尚未全面迁移。
更详细的实验范围、假设与结果见 [tier-2 说明](experiments/tier2/README.md)。
旧 Zobrist、babble、prefix、slotted 等路线的导航见 [research](research/README.md)。

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

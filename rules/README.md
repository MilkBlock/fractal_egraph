# 原生规则实现

- `tier2.egg`：扩展描述、连续重复、坐标变换。
- `higher.egg`：已有组合链折叠为 `HigherRule(k, R, ctx, binding)`。
- `reduce.egg`：显式 `Reduce`、解析归约与成本。

这里是唯一实现位置；`experiments/tier2/*_ir.egg` 和 `ir.egg` 只保留兼容 include。
这些规则扩展 egglog 的用户级数据类型，不构成一个新 egraph 内核。

HigherRule 还依赖 tier-1 的 Comb/RelativeBinding 定义，目前在
`experiments/tier1_effects/tier1_rule_comb_ir.egg`。该文件有未提交修改，本轮未移动或改写。
默认实验入口负责按依赖顺序加载规则；不要重复包含同一个定义文件。

# 原生规则实现

- `tier2.egg`：扩展描述、连续重复、坐标变换。
- `higher.egg`：已有组合链折叠为 `FractalComb(Depth(k), R, ctx, binding)`。
- `fractal_views.egg`：规范化组合视图、实例级段内输出和 effect 地址。
- `reduce.egg`：显式 `Reduce`、解析归约与成本。

这里是唯一实现位置；`experiments/tier2/*_ir.egg` 和 `ir.egg` 只保留兼容 include。
这些规则扩展 egglog 的用户级数据类型，不构成一个新 egraph 内核。

FractalComb 还依赖 tier-1 的 Comb/RelativeBinding 定义，目前在
`experiments/tier1_effects/tier1_rule_comb_ir.egg`。
默认实验入口负责按依赖顺序加载规则；不要重复包含同一个定义文件。

FractalComb 返回 Comb，可以继续作为 SmoothComb/CoarseComb 的父节点。
启动上下文保留在 ctx，重复次数不包含 coarse 注入；只接受有 witness 的 unary SmoothComb 链。
当前仍保留原图，以 CombMeaning/VerifiedView 校验另选的规范化视图，不做全 effect union 或节点删除。
`higher` 命令、ruleset 和 JSON 中 `higher_rules` 字段保留兼容名称；新的 constructor 为 FractalComb。

`fractal_views.input_addresses` 的每项为 `[consumer_event, parent_slot, segment_endpoint_event, iteration]`。
`fact_history` 按 occurrence 和段内位置保存输出端口、行写入标识、union effect；启动注入留在 trigger 对应的原始 occurrence。

`recursive_patterns.egg` 定义共享 RecursivePattern 的步骤/出口、实例证据及输出地址。
所有 FractalComb 共用 ExpansionExtent；线性视图使用 Depth，多出口不均匀展开使用 SparseExtent。
目录排序在 `src/native_catalog.rs` 的观测偏序索引中完成，不将 dominance 当作 egraph 等价关系。

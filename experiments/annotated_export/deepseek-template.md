请将以下已生成的 .egg 接入现有可视化链路：
{{ARTIFACT}}

工作仓库：
- /Users/mineralsteins/Repos/egg_related/eggplant-pattern-web-editor
- /Users/mineralsteins/Repos/egg_related/eggplant_pattern_view_plugin
重点文件：eggplant-pattern-extractor/src/extractor.rs。
实际检查结果：extractor.rs 的 extract_pattern 解析 Rust AST；网页 src/webTranspiler.ts 先用 WASM transpile_egg_to_eggplant 把 .egg 转为 Rust。因此注释必须在 .egg 转译前提取并映射到生成代码/IR，不能假定 Rust extractor 原生读取 .egg 注释。

需要完成：
1. 读取紧贴下一条声明的 `; @egg-viz-json {单行 JSON}`。schema 为 egg-viz/v1。仅作元数据；不要修改 egglog 语法或把 JSON 作为 .egg 指令。无注释、labels 为空或单个节点没有指定标签时，完全沿用现有默认命名。指定标签仅改显示，不改 Rust 标识符、变量绑定或连线。
2. labels.bindings 将源变量名映射到显示名称；同一变量的所有引用一起显示，但不能按显示名称合并节点。labels.positions 使用原生 AST 位置：body/N/expr/J 后可接 /args/K；Eq fact 的 expr/0 和 expr/1 分别是两端，普通 fact 只有 expr/0。head/N/expr/J 对应 Action::Expr/Let 的唯一表达式、Union 的两端；不支持的 action 必须明确报诊断，不能猜测位置。路径区分同一表达式的不同出现位置。应建立 .egg AST→生成 Rust→PatternIr 的稳定 source map。
3. combined_witness_bundle 是带来源与连线见证的组合可视化，不是已证明的快捷规则。保留 stages 分组、connections 的端口约束和所有边界读取。row/rebuild/union 证据可以展开。__viz_combined ruleset 不被调度，不要擅自运行。
4. statistics.observed_occurrences 用于默认降序排列；productive_consumer_occurrences、continued_instances、scopes、next_rule_counts、next_motif_counts 分别展示。后继边数可能大于实例数，不是概率。compiled_macro_executions=0 不能显示成“使用了 N 次”。original_rule 的 statistics 是 match/存活/直接插入/union 等原生计数。
5. 使用已有 .egg→eggplant Rust→Typst→SVG 渲染链路，显示所有原始规则和全部组合。允许按 rule、rank、scope 筛选。保留未标注文件的原有体验。
6. 检查两仓库 AGENTS.md 和工作区，不覆盖未提交工作；不要手改 vendor WASM 来假装完成，若修改了 transpiler 必须找出真实构建源并重建适配的产物。

验收：
- combined.egg 中 {{ORIGINAL_COUNT}} 条 original_rule 和 {{COMBINED_COUNT}} 条 combined_witness_bundle 全部展示，排序和 JSON 一致。
- 中文标签、重复显示名、变量复用、嵌套 position、缺失标签回退、非法 JSON/未知 schema/过期 selector 的诊断均有测试。
- 未标注 .egg 与现有 Rust 示例仍正常；用户字符串不得当作可执行 Typst 或 HTML 注入，应使用现有转义方式。
- 保留所有输入/输出/guard/union；不要把 producer 两个角色 p0/p1 无条件合并。
- 用实际页面与 SVG 渲染验证，报告无法支持的节点/语法，不能静默省略。
- 维护各仓库 Git 历史，提交后报告 commit ID 和复现命令。

本任务不要求生成新的宏规则、证明交换律、做 FlashAttention 推导，也不要求改变 egglog 内核。生成器已验证原程序与导出文件均可由本地 egglog 执行全部原始检查；这不等于现有 WASM/transpiler 已支持文件中的所有构造，需要你实际验证。

本文件统计轮数上限：{{ROUND_LIMIT}}（null 表示没有设置采样上限）。必须展示统计范围，不能把前几轮的频率当作完整原始 schedule 的频率。原始运行轮数不因统计采样改变。
若 ast_connections_complete=false，connections 中的 unresolved_ast_occurrence 只有事件级连接已确认，AST 节点位置尚未唯一确定。展示候选位置或阶段级虚线，不要任选一个节点画实线。同表 read_slot 是 trace 内的序号，不保证等同于源 AST 的前序遍历序号。

已有 compiler_source_span 映射时，优先使用 producer_combined_position / consumer_combined_position 定位整个 combined rule 内的节点；producer_position / consumer_position 是各自源阶段内的相对位置。两者均采用 body/N/expr/J/args/K 协议。位置来自编译期 span 透传到实际读写事件，不能再用 read_slot 或同名操作个数替代。新的统计签名包含连接两端的位置，所以同一对规则作用于内/外层节点会分成不同组合类。

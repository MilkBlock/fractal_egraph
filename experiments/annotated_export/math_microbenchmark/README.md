# math-microbenchmark combined.egg

直接打开本目录的 combined.egg。包含 24 条原始规则（rewrite 经原生 AST
等价展开为显式 rule）和 101 类观察到的组合结构；Math 的 fields、typst、
precedence 保留在 dsl_type 注释中。没有添加 :args_name 等非原生语法。

统计来自前 5 轮的 582 次带 producer 见证的 consumer 事件。每条统计
注释都带 profile_round_limit=5。原始程序和导出的 combined.egg 仍执行
11 轮，打印的全部函数行数已比较一致；Add=641743、Mul=345075。
这不是完整 11 轮的组合频率榜。

组合位于 __viz_combined ruleset，不被调度。它们是保留各阶段条件和动作
的可视化见证组合，不是可替换原始推导的快捷规则。

31 类组合的 AST 连接可唯一确定；70 类含未唯一定位的连接。数学规则
中同一 LHS/RHS 经常有多个 Add/Mul；trace 的同表读取序号不保证对应
AST 前序出现位置。因此对这种情况保留真实事件连接和候选表达式，
标记 ast_connections_complete=false，不生成猜测的端口等式。下游应
显示阶段级连接或候选虚线，不应任意挑选一个节点。

复现：

```
CARGO_INCREMENTAL=0 cargo build --release --bin combine_profile --bin export_combined_egg
target/release/combine_profile experiments/annotated_export/math_microbenchmark/source.egg experiments/annotated_export/math_microbenchmark/profile.json 5
target/release/export_combined_egg experiments/annotated_export/math_microbenchmark/source.egg experiments/annotated_export/math_microbenchmark/profile.json experiments/annotated_export/math_microbenchmark
```

source.egg 是原 microbenchmark 加显示注释；profile.json 是真实事件统计；
DEEPSEEK.md 包含本文件对应的规则数量、采样范围和渲染要求。当前交付
只验证原生解析、运行、表大小一致和导出协议，没有验证网页/Typst SVG。

dsl_type 的 `typst` 与 `precedence` 与 `samples/math_microbenchmark.rs` 中的
`#[eggplant::typst(..)]` / `#[eggplant::precedence(..)]` 保持一致（占位符改用本
文件的字段名），因此 .egg 与 .rs 两条链路渲染出同一套数学记法：`{left} + {right}`、
`frac(..)`、`{base}^{exponent}`、`integral {integrand} quad d {variable}`、
`{expression}'({variable})`，优先级 Add/Sub 50、Mul/Div 60、Pow 80、其余 90/100。
模板只写自然形式，`upright(..)` 由渲染器自动插入。

离线核对（125 条规则、1647 个 typst 目标、0 失败）：

```
cd dpsk_workspace/viz-web-editor
npm run render:rules -- --egg ../../experiments/annotated_export/math_microbenchmark/combined.egg --out /tmp/rule-mm
```

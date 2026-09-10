# 带显示命名与统计注释的组合 .egg 导出

本任务仅生成交换文件和 DeepSeek 交接提示词；不修改 web editor、extractor
或 transpiler，也不声称已生成 SVG。

## 运行

```
CARGO_INCREMENTAL=0 cargo run --bin export_combined_egg -- \
  egglog/tests/web-demo/cyk.egg experiments/combine_profile/cyk.json \
  experiments/annotated_export/cyk experiments/annotated_export/labels.json
CARGO_INCREMENTAL=0 cargo test --bin export_combined_egg
```

输出 `cyk/combined.egg` 和 `cyk/DEEPSEEK.md`。最后的标签 JSON 参数可省略。
源码中已有的同格式注释也会被读取；命令行标签覆盖相同 rule ID 的标签。
源文件不被改写，导出副本通过原生 AST 格式化，普通说明性注释不保留。

## 注释约定

紧贴声明之前：

```egg
; @egg-viz-json {"schema":"egg-viz/v1","id":"R1","kind":"original_rule","labels":{"bindings":{"p1":"左区间长度"},"positions":{"body/1/expr/0":"左区间事实"}},"statistics":{}}
```

JSON 必须为单行；换行、引号等由 JSON 编码转义。egglog 将整行视为注释。
注释解析是可视化协议，不增加或修改 .egg 的语言语法。

- `bindings` 是源变量到显示文本的映射，不重命名变量、不改变 binding。
- `positions` 是出现位置到显示文本的映射，区别于“同变量的所有引用”。
- `body/N/expr/J/args/K/...`：原生 Rule.body 的第 N 个 fact；普通 fact
  的 expr/0，Eq 的 expr/0 和 expr/1；args 为零起始的 Call 参数路径。
- `head/N/expr/J/args/K/...`：第 N 个 action；Expr/Let 只有 expr/0，
  Union 有两端。其他 action 的位置命名暂不支持，会明确拒绝。
- 缺注释、缺 labels、空映射、或某一节点缺标签：下游保持原有默认命名。
  目前已检查 extractor 默认节点 id 来自 Rust let 绑定，label 为
  `name: dsl_type`；不要在缺注释时另造一套命名规则。
- `statistics` 保留 profile 原始字段，排名按 observed_occurrences；它
  不是已执行宏规则次数。出现次数、productive 次数与后继边数要分开。
- 组合中的 `stages` 给出 producer/consumer 的 body/head 范围，`connections`
  给出追加的端口相等约束以及 read/write/rebuild/union 见证。

## 组合是什么

当前 profile 的八类是历史生产—消费结构，不是已证明的快捷 rewrite。
导出器为每个 producer 实例分配 p0/p1 命名空间，consumer 分配 c；重复
使用同一 producer 实例时只复制一次。保留所有阶段 LHS 与动作，再把
被消费的表调用与对应 producer 输出的键逐列用等式连接。

因此 kind 为 `combined_witness_bundle`，status 为
`visualization_only_not_a_shortcut`。消费者的中间读取仍留在 LHS，不能
把该文件用于证明已经消除了中间匹配。各阶段标签自动按命名空间和
位置偏移传递，不在作用域外假设变量相同。

所有组合放入独立 `__viz_combined` ruleset，原始 schedules 不运行它。
导出器分别用本地 egglog 执行原程序和完整导出文件，全部原始检查通过后
才写出产物。组合定义也经过原生类型检查。这不证明旧版 WASM 已支持
所有语法，更不证明组合是一条可替换原规则的等价快捷路径。

当前版本服务于本轮 CYK 的直接 Rule profile：支持 Expr/Let/Union 内
唯一可识别输出表的连接；同一 producer 有多个同名输出调用时明确报错。
不猜测输出位置，不自动组合任意关系规则、rewrite 或跨作用域定义。
源 rule 定义与 profile 不匹配时要求重新统计，不用旧数据贴新程序。

## 下游真实入口

`eggplant-pattern-extractor/src/extractor.rs` 的 extract_pattern 使用
ra_ap_syntax 解析 Rust，不是 .egg parser。网页 src/webTranspiler.ts 先调用
vendor WASM 的 transpile_egg_to_eggplant。所以 DeepSeek 必须在 .egg 注释
仍存在时提取元数据，并通过 source map 传到生成 Rust/PatternIr，再由
已有 Typst→SVG 流程渲染。完整工作提示词由 deepseek-template.md 生成，
含输入路径、协议、默认回退、统计含义和验收测试。

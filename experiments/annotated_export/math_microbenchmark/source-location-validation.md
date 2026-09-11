# 源位置映射验证

本轮在真实 egglog 编译与提交路径上修复映射，没有用当前图反推历史位置。

- LHS：原始 atom span → bridge atom → cached query plan → RowReadEvent。
- RHS：core action span → 仅依赖模式启用的 TraceSource 指令 → 暂存写入
  的 TraceCause → 实际 WriteEvent。每个 action lane 清空/恢复源位置状态。
- union-to-set：原来使用整条 union/rule 的 span，现在使用被物化的
  constructor call span，区分嵌套 RHS 的内层与外层 Add。
- rebuild：来源位置随原来的 row provenance 一同传递。
- Profile：将源文件 byte range 映射回 normalized Rule AST 路径，并将
  两端路径纳入组合签名。每个映射还验证操作名与 witness 的 table 一致。
- Export：输出源阶段路径和整个 combined rule 路径；不再用同表 read_slot
  或同名调用数量做推断。没有源数据或存在多位置时仍保留候选并明确标记。

## 当前 math 数据

同一份前 5 轮数据：582 个带 producer 支持的 consumer 事件、834 条连接。
834 条连接均为 1 个 producer site 和 1 个 consumer site。旧 101 个仅按
规则/表槽位区分的类别，细分成 124 个端口类别；所有导出组合均为
ast_connections_complete=true。并非增加了样本或宣称普适语义等价。

源位置指向这次编译的输入，不是跨文件版本的全局位置 ID。宏展开或低层
API 缺少源信息、旧 profile 缺少字段时，不保证可定位；不能将缺失信息
补成猜测。实际保留的 producer 是提交时的来源，不枚举同一事实所有
可能推导。本文结论只覆盖已验证的 math 数据和回归用例。

## 验证

- source_locations：嵌套 LHS/RHS Add、union-to-set 位置、多个 lane 隔离。
- union_trace：rebuild 后 source_span 与原始写入一致。
- dependency_trace / program_trace：既有真实依赖与原生 schedule 回归。
- source_mapping：每条 math 连接都有唯一位置；两端的 combined AST 路径
  均可实际解析到正确的操作节点；新组合数大于旧粗粒度类别数。
- 导出器再次执行原文件及 combined.egg 的完整 11 轮，所有打印表大小一致。
- 现有 render:rules 链路成功处理 148 条规则（24 + 124），共 2133/2133
  个 Typst 目标，0 失败。产物在 /tmp/rule-mm-source-sites。这里只验证
  离线渲染成功，不声称已经改造前端所有连接交互。

```
CARGO_INCREMENTAL=0 cargo test --manifest-path egglog/Cargo.toml -p egglog --no-default-features --test source_locations --test dependency_trace --test union_trace --test program_trace
CARGO_INCREMENTAL=0 cargo test --test source_mapping --bin combine_profile --bin export_combined_egg
cd dpsk_workspace/viz-web-editor
npm run render:rules -- --egg ../../experiments/annotated_export/math_microbenchmark/combined.egg --out /tmp/rule-mm-source-sites
```

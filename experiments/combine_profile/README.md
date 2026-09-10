# Rule combination 结构分析

研究目的转为：发现反复出现的规则执行结构、数据绑定接口与后继组合，
为宏规则抽象、规则层的不动点/饱和和结构共享提供证据。执行时间只是
后续工程代价；不是此实验的优化目标。

## 现有实现实际维护的数据

- RuleSpec 目录：名字、LHS/RHS，及可选无条件 rewrite 模板。
- TraceSession 共享事件库：match bindings、完整行 witness 标记、action
  outcome、committed writes、union、rebuild 版本、失效和作用域事件。
- DependencyBlockStore 共享索引：match/read/write/output 映射，fact/match
  的 block owner，依赖与交互边，by_rule 的交互索引。
- 每个已观察 match 的 Recipe：模板、binding 环境、成员及支撑 reads。
  BlockMeta 保存入口、成员、边界、prefix、版本与有效性。

这些并不是每条规则各自复制一份历史。目前没有永久维护每条 rule 的
完整 binding join 表，没有频率驱动的选择/晋升器，也没有宏规则层的
e-graph。此前 binding_reuse/residual_reuse/union_dirty 是独立实验索引。

## 新增的分析表

combine_profile 复用原生 parser/run_program_with_trace，给未命名规则加
R0/R1/R2 标签，不改变任何动作、schedule 或检查。完整 CYK 的三个场景
同时用于识别递归、自消费、多来源汇入和 union/rebuild 的支撑关系。
选择它是因为已有可复现的 trace 支持；它不代表 FlashAttention 的算子语义。

输出包括：

1. 每 rule 的候选、存活、直接插入及 union 数。
2. 按读取表和槽位区分的 producer-consumer 配对频率。
3. 按 consumer 事件计数的多 producer 组合结构排名。
4. 每个结构的产生新事实/union次数、出现作用域和示例事件。
5. 该组合实例后面接什么 rule、什么组合结构，以及后继边数。
6. 可追溯到真实 read/write/match ID 的全部证据。

“产生新结果”目前仅计直接 Inserted 或 changed Union，不计 Updated。
统计使用读取发生时的真实来源，即使来源后来因 rebuild 失效也保留历史
支持；拒绝跨 scope 的来源复用。缺 producer 的读取保留为边界，不能
因此把局部结构声称为闭合 combined rule。

结构签名保留 producer 实例是否相同、读取表和同表序号。row 与有证据
的 rebuild 来源归入相同规则结构，但详细证据保留 rebuild 与 union IDs。
它不是完整带类型的 binding 同构分类：没有把所有常量、guard 和端口
相等关系都纳入等价判定，不能直接依据签名替换程序。

观察频率 != 编译候选数量 != 实际宏规则使用次数。本程序只运行原始
三条规则，因此 executed_compiled_macros = 0；selection_policy = none。
下一步晋升宏规则时才应记录 attempt/accept/reject 与理由及实例范围。

## 复现

```
CARGO_INCREMENTAL=0 cargo test --bin combine_profile
CARGO_INCREMENTAL=0 cargo run --bin combine_profile -- egglog/tests/web-demo/cyk.egg experiments/combine_profile/cyk.json
python3 experiments/combine_profile/render.py
```

阅读 cyk.md 的排序表，cyk.json 保存原始统计和实例证据。组合分布不能
直接证明普适性；后续应在独立规则集与输入上验证，而不是仅重复这三个
作用域。本实验未建立 FlashAttention 推导，也未证明宏规则交换律。


来源频率还受 schedule 和提交时的去重赢家影响：记录的是实际保留的
producer 见证，不是同一事实的所有可能推导。不能将某个来源未出现解释
为该规则组合不存在，也不能把一个样本中的频率当成普适选择概率。
JSON 中的 binding/key/row 值是运行时 Value 编号，须结合原生表 schema
解释；它们没有被当成跨类型或跨运行的结构同构证据。

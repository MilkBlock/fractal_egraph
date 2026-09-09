# 覆盖账本与可重叠 prefix 视图

本阶段实现 `CoverageLedger`，将逻辑 membership 与原生存储归属分开。它是诊断账本，不改变 egraph 推理、节点存储或匹配去重策略，也不保证已经实现均匀探索。

## 分母从哪里来

`CoverageLedger::new(function_names)` 明确指定覆盖域。`refresh()` 枚举这些函数表的所有活跃、未 subsume 行，包括初始输入与没有发生 apply 的冷区。覆盖域之外的 primitive 内部存储、索引、未选择的表等不计入分母。

事实身份是函数名与其逻辑行字段；所有权始终是原生函数表。行退出当前库存后成为 `Retired`，再次出现会分配新账本 ID。库存轮询不能完整识别两次轮询之间所有物理版本变化，来源失效仍需由 dependency trace/block store 处理。

覆盖口径是 prefix 执行中的构造器行读写足迹，不是从入口递归可达的整张子图，也不是所有未来规则的计算覆盖率。slot 指向某个 class，不自动意味着其内部节点已被覆盖。

## 多重 membership

只从当前 `DependencyBlockStore` 已确认且可用的组合 prefix 发布视图。视图覆盖其成员事件实际读取和插入的当前行，保留独立的 view ID、来源 block 和 prefix 索引。

```text
view 0: A+B       → {A, B, C}
view 1: A+B+C     → {A, B, C, D}

membership 数：3 + 4 = 7
覆盖事实数：|{A, B, C} ∪ {A, B, C, D}| = 4
```

共享的 B 事实有两个 membership，但仍只存于原生 B 表中。发布视图不会复制或迁移它。

`marginal_new_facts(view, selected_views)` 计算相对于已选择视图的新增覆盖。上述长视图相对短视图只增加一个事实，短视图相对长视图不增加事实。

当前示例使用已存在组合器生成的嵌套重叠。任意滑动边界和依赖 DAG prefix 的产生仍属于后续工作；账本本身可以记录多个视图的交叠，不负责寻找所有视图。

## 覆盖状态

|状态|含义|
|---|---|
|Covered|至少一个有效视图覆盖当前事实|
|SeedOnly|有相关读写见证，但尚无有效组合视图|
|Unobserved|没有 prefix 相关见证；不表示库存扫描没有读到该行|
|Unsupported|相关规则被 block store 拒绝，并记录原因|
|Invalidated|曾被视图覆盖，目前已无有效覆盖|
|Retired|已退出活跃库存，不再计入覆盖率分母|

状态按有效覆盖优先判断。例如一个视图失效，但另一个视图仍覆盖 B，B 仍是 Covered。失效原因、seed 标记等信息可以同时存在，不能把该状态理解为所有使用方式都已被解释。

显式撤回一个视图只改变它自己的有效性，不改变其他视图。真正的成员行退出库存，会使引用该成员的视图失效。来源失效使 block/prefix 不可用时，同步也会使其视图失效；不会自动恢复旧视图。

## 计费与探索接口

- 事实覆盖和逻辑 payload 按并集计算，重叠不重复计费。
- payload 为函数参数及返回值的 Value 字段字节数，不包括全部物理行元数据、allocator 开销或索引，**不是实际内存或压缩节省**。
- `uncovered()` 返回当前未覆盖的事实；`mark_examined()` 记录显式检查次序，为后续探索策略预留接口。
- 当前没有抽样器、探索配额或公平性保证。`refresh()` 是全量库存扫描，事件同步读取累计 trace，因此也没有低开销维护声明。

## 实际示例

输入含 `A(Var 7)` 和不参与规则的 `Cold(Var 99)`，规则为 A→B、B→C、C→D。

|阶段|活跃行|有效覆盖|membership 数|
|---|---:|---:|---:|
|初始库存|4|0|0|
|只执行 a|5|0|0|
|执行 b|6|3|3|
|执行 c|7|4|7|
|撤回长视图|7|3|3|
|删除 B|6|0|0|

三个未覆盖的行是 Var 输入和 Cold 区域，不会因为只查看有 trace 的数据就消失。阶段快照在 `results/coverage/examples.json`。

## API 顺序

```rust
let mut coverage = CoverageLedger::new(function_names);
coverage.refresh(&egraph)?; // 包括未发生 apply 的初始库存

egraph.step_rules_with_trace("a", &trace)?;
blocks.ingest_committed(&trace)?;
coverage.sync_blocks(&egraph, &blocks, &trace)?;
```

同步要求 block store 和 coverage 属于同一 trace/execution lineage。来源失效不能仅靠 `refresh()` 代替处理；应先更新 block store。规则定义、函数 schema 和数据库分支也不能随意混用。

内核新增诊断 table-name registry，使没有后续 consumer 的最终 RHS 写入也能映射到库存，而不是只统计已经被再次读取的结果。

## 验证与复现

```sh
CARGO_INCREMENTAL=0 cargo test --test coverage --test dependency_blocks
CARGO_INCREMENTAL=0 cargo run --bin coverage
```

测试覆盖初始/冷区库存、SeedOnly、重叠并集、单一原生 owner、边际覆盖、单视图失效隔离、实际删除、重插入不恢复旧视图和未知规则原因。没有安装快捷规则、创建物理分区或引入 Zobrist 摘要。

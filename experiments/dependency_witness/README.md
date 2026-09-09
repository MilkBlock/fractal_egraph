# 实际提交结果 → 消费匹配 → Combined Prefix

本阶段修改的是本地 egglog 内核，新增显式启用的行依赖诊断模式。它将 match 的 cause ID 随动作缓冲区传到真实表提交点，再将后续匹配读取的行连接到插入者。没有通过前后快照差分、规则执行顺序或 Value 相似度推测 producer。

## 已实现的链路

```text
A-to-B 的 match
    ↓ cause 随 RHS 写入进入分片缓冲区
SortedWritesTable 判定 Inserted
    ↓ 保存该 key 对应当前完整行的来源
B-to-C 的 match 用完整 key 定位实际 LHS 行
    ↓ 读取来源，并核对当前行版本
producer match → committed write → row read → consumer match
    ↓
经过两步组合器生成 A+B prefix seed
```

构造器示例使用真实 rewrite：

```text
A-to-B: A(x) → B(x)
B-to-C: B(x) → C(x)

被证据链支持的组合：A(x) → C(x)
中间项：B(x)
```

在 `results/dependency_witness/examples.json` 的 fresh 场景中，可查看具体 producer match、提交事件、读取事件和 consumer match 的 ID，以及表名、key、实际行与组合 LHS/RHS。竞争场景只沿真实提交胜者生成 prefix；B 早已存在或根本没有 A 匹配时，不会生成这条 A+B 行依赖。

这些 ID 是单个 trace session 内的标识，不是跨并行 worker 的全局因果排序。来源只表示这次诊断执行中实际插入该行的事件，不表示它是唯一可行证明或反事实意义下必不可少的原因。

## API

```rust
let trace = egglog::TraceSession::with_dependencies();
egraph.step_rules_with_trace("source", &trace)?;
egraph.step_rules_with_trace("consumer", &trace)?;

let matches = trace.matches();
let writes = trace.write_events();
let reads = trace.row_reads();
```

`WriteOutcome`：

|结果|含义|
|---|---|
|Inserted|表提交时 key 原本不存在，真实加入新行；可建立行来源|
|Deduplicated|已经提交的行保留，当前写入没有改变它；不会替换原 producer|
|Updated|merge callback 改变了已有行；移除旧来源，不假设它是单一 producer 的新事实|
|Unsupported|不支持提交归因的表在 staging 时发出的通知；不是已提交证明|

预测缓存直接返回已有值时，可能没有新的 staged write，因此也不会产生相应提交事件。`write_events()` 不是“每次 RHS 操作都恰好一条事件”的计数器。

`RowReadEvent` 保存 table、table_name、完整 key、实际 row，以及可选的 `producer_match_event_id` 和 `producer_write_event_id`。只有来源属于同一 trace session，且记录的行与当前行逐值一致，才返回 producer。`None` 表示未知或外部输入，不是“没有任何依赖”的证明。

## 如何取得 LHS 见证

编译规则时保存每个显式 LHS atom 的 key 描述，支持常量 key 和保留的变量。匹配到达动作边界时，数据库仍是只读视图：若完整 key 可用，则通过唯一键定位实际行并读取来源。

如果 key 变量被查询规划器消去了，或该表不支持具体行见证，不猜测哪一行被使用，`physical_witness_complete` 保持 false。该字段为 true 仅表示显式 LHS 表 atom 的 keyed-row 见证齐全，不代表 union、外部 primitive 或所有语义前提的证明齐全。

## 生命周期与覆盖边界

- 来源跟随行缓冲区穿过分片及延迟提交，并在真实插入/merge 分支上记录结果。
- 覆盖一个已有 key 的行会使旧来源失效。
- 当前实现对出现 remove 的表保守清空来源索引，包含 rebuild 的 remove/reinsert；宁可漏掉依赖，不保留过期来源。
- clone 复制来源状态；新 trace session 不会把另一 session 的 event ID 误认成自己的 producer。
- **尚未追踪 union 的证明链和 congruence/rebuild 的完整传递来源。** 测试专门覆盖“B 只有在 union 后才能匹配”的情况，当前不把它伪装成 A 插入了一行。
- 没有安装快捷规则或建立 block，也没有压缩节点。输出是供下一步使用的、有具体行依赖证据的 combined-prefix seed。

## 这是诊断模式，不是低成本生产实现

只有 `TraceSession::with_dependencies()` 开启行来源记录；普通 `TraceSession::new()` 保留原逻辑 trace 行为。

为了精确关联单个 match 与写入，诊断模式逐 lane 执行动作；携带来源的表使用串行提交，查询仍可并行。与普通向量化执行相比，动作交错顺序可能变化，因此语义对照限定为本实验的纯关系／构造器规则，不对有外部副作用、顺序敏感或早停的规则作等价承诺。

表一旦携带来源，后续提交继续使用串行路径来正确维护失效，直到 clear。规则保存了额外 key 描述，已跟踪的行持有来源引用，trace 保留事件记录；没有完成内存上限和低开销优化。此阶段不能用于宣称压缩或推理加速。

## 验证

新增集成测试覆盖：

- 新关系行和新构造器行被后续匹配消费；
- RHS 预先存在，不冒领 producer；
- 两个规则竞争写入，只认提交胜者；
- 覆盖、删除及无 trace 的重插入使旧来源失效；
- 常量 key、被消去的 key 变量和默认 trace 模式；
- union 才启用匹配的未覆盖情况；
- 300 行跨 batch 对照；
- 20,001 行、4 个 worker 的并行搜索与普通执行对照，并逐条核对读写证据。

同时运行原 core-relations、bridge 和 trace integration 回归测试。

## 复现

```sh
CARGO_INCREMENTAL=0 cargo test --manifest-path egglog/Cargo.toml \
  -p egglog --no-default-features --test dependency_trace --test trace_instrumentation
CARGO_INCREMENTAL=0 cargo test --manifest-path egglog/Cargo.toml \
  -p egglog-core-relations --lib
CARGO_INCREMENTAL=0 cargo test --manifest-path egglog/Cargo.toml \
  -p egglog-bridge --lib
CARGO_INCREMENTAL=0 cargo run --bin dependency_witness
```

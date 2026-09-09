# DependencyBlockStore：依赖驱动的活跃 block 索引

这一阶段建立的是**真实执行事件上的诊断 block 索引**。它使用已提交行的 producer–consumer 见证决定归属，生成块内组合 prefix，并记录跨块交互；尚未迁移 e-node 存储、限制原生匹配扫描或安装局部快捷规则，不能据此声称内存或推理时间下降。

## 形成与增长

单次 match 只保存为待连接种子。只有后续匹配读取其真实 Inserted 结果，并且匹配、提交行、读取行和来源 ID 一致，才形成 block。

```text
执行 a: A(x) → B(x)
    只有种子，没有 block

执行 b: B(x) → C(x)，读取 a 提交的 B(x)
    block 0 = [a, b]
    入口 A(x)，组合候选 A(x) → C(x)

执行 c: C(x) → D(x)，读取 b 提交的 C(x)
    block 0 = [a, b, c]
    同一个 handle，成员容量 2 → 4
    新组合候选 A(x) → D(x)
```

成员容量是真实成员数组的预留空间，按需 `reserve_exact` 扩容，BlockId 不变。但它不是 egraph 物理节点 arena 的容量。

fork 不会被伪装成串行依赖：若后来的 f 读取的是 a 产生的 B(x)，只产生 `[a,f]` 的组合候选，不把已经发生的 b、c 强行插入这条推导。

## block 保存什么

- 稳定 ID、版本、active 状态与成员容量；
- 已注册源规则的 LHS prefix、具体匹配 ID 和入口绑定；
- 已确认的成员匹配、内部读写依赖；
- 外部输入或未知来源的读取见证，以及跨块读取；
- 潜在对外可见的全部有效产出；
- 组合 prefix 的符号表达式、具体绑定、所覆盖的成员序列和支撑读取事件。

不会因为某个结果已在块内被消费，就立即把它从接口中删掉；未来的其他块仍可能读取它。

`block_for_match()` 和 `block_for_fact()` 通过索引定位 block。后者的参数是实际提交事件 ID；这还不是直接附着于任意 e-node 的 block 字段。`active_block_for_fact()` 进一步检查来源和块状态。

## 跨块交互

当一个 consumer 同时使用两个 block 的结果时：

```text
block 0: LS → LM → LL ─┐
                       ├─ join(LL, RR) → Out  [block 2]
block 1: RS → RM → RR ─┘
```

保留原来的两个 block，创建交汇 block，并记录带见证的有向超边。提供按 block 和按 rule 查询 interaction 的索引。容量不足不会导致自动拆块；跨块消费也不会无条件吞并所有父块。

如果多个尚未形成 block 的 producer 共同被消费，会先把它们作为交互源段显式登记。这些源段可能暂时只有一个成员，但已经有真实消费边，而不是仅凭普通 match 创建。

## 组合前缀的约束

`RuleSpec::rewrite` 只接受调用方注册的真实无条件等式 rewrite，使用现有组合器继续构造组合。当前还要求入口结构能对齐具体绑定，且实际读取的表和 key 对应组合的重叠位置。需要进一步展开变量内部结构才能对齐的情况会跳过，不猜测绑定。

`RuleSpec::opaque` 用于多 atom、条件或关系规则的明确入口描述。它允许记录依赖和交互，但不进入等价规则组合器，避免把关系推导当成等式 rewrite。

每个组合 prefix 只声明自己覆盖的成员序列，不宣称等同于包含其他分支的整个 block。所有候选都尚未安装。

规则名称必须在该 trace 中唯一且定义不可变；注册描述必须与引擎实际规则一致。后续应直接接入编译后的规则身份和 AST，当前不支持名称复用或事后更换定义。

## 失效

内核新增 `origin_invalidations()`：覆盖、remove/rebuild 或 clear 使来源元数据失效时，通知原 trace session。它表示来源证明不再可用，不等于所有涉及行都被物理删除。

索引将所属 block 标记为非 active，并沿交互边使下游见证失效。符号等价规则本身不因此变错，但其具体 prefix 实例不能继续当成已验证状态使用。

- 未知规则、缺失绑定、不完整 keyed witnesses 和失效 producer 不会被吸收到块中。
- 一个 join 被拒绝时，不会顺便把另一个无关种子建成孤立 block。
- 未跟踪的 union 或其他外部修改需要调用 `invalidate_all()`；完整 union lineage 尚未覆盖。
- 不会自动删除既有 egraph 结果；自动重新验证、重新激活以及成员迁移也尚未实现。

## API

```rust
let mut blocks = DependencyBlockStore::new(specs);
let trace = TraceSession::with_dependencies();

// 只在整个 step 已经返回、提交完成后摄取事件。
egraph.step_rules_with_trace("a", &trace)?;
blocks.ingest_committed(&trace)?;
egraph.step_rules_with_trace("b", &trace)?;
blocks.ingest_committed(&trace)?;
```

重复摄取同一 trace 是幂等的。一个 store 绑定一个 trace/execution lineage；拒绝混入其他 session。不要用同一 store 同时管理分叉的多个数据库实例。

事件摄取目前会读取 trace 的累计快照，再按 ID 去重；不是低开销的流式实现。诊断 trace 的逐 lane 动作与串行提交成本仍然存在，且索引保留历史。Zobrist、原生局部匹配入口、事件回收和性能消融均未混入本阶段。

## 测试与示例

```sh
CARGO_INCREMENTAL=0 cargo test --test dependency_blocks
CARGO_INCREMENTAL=0 cargo test --manifest-path egglog/Cargo.toml \
  -p egglog --no-default-features --test dependency_trace --test trace_instrumentation
CARGO_INCREMENTAL=0 cargo run --bin dependency_blocks
```

`results/dependency_blocks/examples.json` 保存每个实际规则 step 后的 block 快照，包括链增长、双块交互和删除后的失效传播。

测试覆盖稳定句柄与扩容、三步组合、fork、交互超边、删除和覆盖失效、未知修改、session 隔离、预先存在的 RHS、不注册的规则，以及拒绝 join 时不产生孤块。

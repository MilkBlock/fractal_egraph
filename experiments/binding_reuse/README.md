# Binding 复用的第一组消融

结论：在这组模型里，来源驱动可以正确重建 binding，但**没有比普通增量
join 少枚举候选**。紧凑 binding 表示在高扇出情况下有明显的编码空间收益，
但没有消除必须输出的事实，也不等于 e-graph 内存压缩。

## 实际做了什么

在真实 egglog 上运行普通执行与 dependency-traced 执行：

```
make-p: SeedP(x,y) -> P(y,x)
make-q: SeedQ(z,y) -> Q(y,z)
consume: P(y,x), Q(y,z) -> Out(x,y,z)
```

smooth 情况使用 `P(y,x) -> Out(x,y,0)`；guard 情况增加 `x < z`。
每个场景分 8 波输入，两侧交错到达。Seed 被故意重复断言，但通知仅取
P/Q 的真实 `Inserted` 写入。初始事实场景显式导入没有 producer 的输入。

普通与 traced egglog 在**每一波**都与独立数学枚举逐元组核对，检查 Out
总行数及所有期望行，排除多出或漏掉结果。再把已提交通知交给独立 Rust
回放算法，四种处理方式每波都必须得到相同集合：

1. full_rescan：每波扫描所有已有 P×Q，弱基线。
2. indexed_rescan：保留 Q 的键索引，但每波重新遍历所有已有 P。
3. delta_join：两侧索引由新增事实驱动；模拟普通增量 join，强回放基线。
4. provenance_dispatch：从真实 producer match 的父 binding 和固定端口
   映射 `(payload,key) -> (key,payload)` 重建通知，再执行相同的 residual join。
   初始事实走显式回退。

后两项复用 join 实现，只开关来源/父 binding 重建，目的是隔离来源信息
本身的增量价值；这没有实现自动发现复杂组合，也没有替换 egglog 的 matcher。
本实验不包含 union、删除、复杂多规则 prefix 或未知连接模板的学习。

## 候选 binding 数（包含被 guard 拒绝的候选）

| 场景 | 索引重扫 | 增量 join 回放 | 来源驱动回放 | 最终结果 |
|---|---:|---:|---:|---:|
| smooth 参数重排 | 1152 | 256 | 256 | 256 |
| coarse，每 key 一个匹配 | 928 | 256 | 256 | 256 |
| coarse，每 key 8×8 | 6528 | 2048 | 2048 | 2048 |
| coarse，单热点 key 256×256 | 208896 | 65536 | 65536 | 65536 |
| coarse，加 guard | 6528 | 2048 | 2048 | 28 |
| 含初始事实 | 6528 | 2048 | 2048 | 2048 |

smooth 两种增量方案都无需历史查询索引。coarse 两种增量方案都进行 512 次
键查询。多对多、热点、guard 场景的 512 条输入均带来源，覆盖率 100%，
仍然未减少强基线的候选数。覆盖率不能代替实际搜索成本指标。

## Binding 编码

实现了按共享 key 存两侧 payload 数组的表示，具有固定宽度的长度头及
偏移表；实际编码、解码，再枚举核对所有 bindings。guard 场景保留筛选
程序，不能把未筛选乘积当成最终输出。该表示是独立快照布局，未接入
内核存储，也没有实现其增量维护。

热点场景：65,536 个结果三元组的固定宽度编码为 1,572,864 字节；
共享表示为 4,160 字节。但如果必须产生 65,536 个不同 Out 事实，仍需要
逐一输出它们。每 key 一个匹配的场景共享表示为 14,360 字节，反而比
平铺 6,144 字节更大；选择性 guard 只留下 28 行，平铺仅需 672 字节。

来源信息单独记账：每个 producer 保存 event ID、rule ID、两个父绑定值，
实际编码为 32 字节，512 行增加 16,384 字节。这还不包含 Rust 容器开销、
原 egraph、完整 trace 日志或 allocator 开销。结果 JSON 另列执行索引的
固定布局估计。所有这些数字都是编码字节，**不是峰值堆内存或 RSS**。

## 复现与边界

```
CARGO_INCREMENTAL=0 cargo test --bin binding_reuse
CARGO_INCREMENTAL=0 cargo run --bin binding_reuse -- experiments/binding_reuse/results.json
```

结果保存了 ordinary/traced egglog 的单次运行时间，以及四种回放算法的
七次运行中位数。当前产物来自 debug 构建，时间仅用于诊断；回放不包含
完整 rule matching、mutation、rebuild、日志收集和通知维护，因此不能
用它与原生时间相除来宣称端到端加速。

下一组有区分力的实验，应使用有多个备选生产路径和 residual 查询的真实
规则集合，检验来源学习能否比已知键索引进一步缩小 residual 搜索。
当前结果支持端口继承的正确性与高扇出 binding 的可因子化性；不支持
“仅加来源表便优于已有增量 join”或“已降低 e-graph 峰值内存”的结论。

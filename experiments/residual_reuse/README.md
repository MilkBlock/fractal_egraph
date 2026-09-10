# 来源信息能否缩小 residual 搜索：第二组验证

结论：来源配对能比单键索引缩小候选，但没有超过利用完整逻辑连接键的
复合索引。训练时的 100% 覆盖在外部输入和 union 后会失效；来源标签不变，
可匹配的来源组合却会改变。

## 原生程序与训练/验证划分

实验使用 4 个 key、8 个 Group e-class，以及 16 条有名称的 producer：

```
make-p-g: SeedP(k,g,x) -> P(k,Tag(g),x)
make-q-g: SeedQ(k,g,z) -> Q(k,Tag(g),z)
consume: P(k,c,x), Q(k,c,z) -> Out(k,x,z)
```

仅从原生 `Inserted` P/Q 写入和对应真实 rule 名称给事实绑定来源。
四个阶段依次是：

1. 每 key、每 group 各一个 P/Q。根据此时经过原生执行核验的有效匹配，
   学到 8 个 producer 配对。模型此后冻结，不使用验证阶段的结果重训练。
2. 每桶扩展到 4×4，测试未见过的 binding。
3. 直接加入 8 条外部 P/Q，没有 producer 来源。
4. 执行真实 `(union (Tag 0) (Tag 1))`，然后运行 consumer。

普通 egglog 和 dependency-traced egglog 在每个阶段均核对 Out 总行数及
每个期望元组；期望结果由独立输入清单及显式 union 语义枚举，排除遗漏与
额外结果。并使用原生 canonicalization 更新原来的事实句柄。

## 五种 residual-search 策略

- key_only：按 k 定位 Q，再逐对检查 group 的当前等价性。
- canonical_composite：按 `(k, canonical(group))` 定位 Q，强索引基线。
- learned_source_only：按 `(k, producer rule)` 索引和已学配对定位 Q，
  再检查实际等价性；没有已学来源的输入不处理。这是故意不安全的消融。
- learned_source_with_fallback：使用完整复合索引提供安全候选集，再分成
  已学来源快速路径和其余回退。两条路径互斥，不重复计算候选。
- stale_composite：保留 union 前的索引等价关系，作为缺失失效维护的反例。

这些是同一快照上的独立 Rust residual-search 回放，**不是已经替换内核
的增量 matcher**。具体 producer-pair 模板已知，实验只学习配对，不学习
任意嵌套 prefix。源码中的安全版本以复合索引实现回退，因此本身不可能
在候选数上超过该基线；比较用来明确来源增加了什么、遗漏了什么。

## 结果

| 阶段 | 单键候选 | 复合索引候选 | 仅已学来源候选 | 仅来源漏匹配 | 安全版本回退 |
|---|---:|---:|---:|---:|---:|
| 训练 | 256 | 32 | 32 | 0 | 0 |
| 扇出扩展 | 4096 | 512 | 512 | 0 | 0 |
| 外部事实 | 4356 | 548 | 512 | 36 | 36 |
| union 后 | 4356 | 676 | 512 | 164 | 164 |

union 新增 128 个有效匹配。原有来源信息不变，但原来不相等的 group 现在
相等，所以仅学同 group 来源配对不足以覆盖它们。164 个遗漏是先前外部
输入造成的 36 个加上新增的 128 个。陈旧复合索引只漏掉这 128 个。
安全版本与完整复合索引每阶段结果完全一致，没有误报或遗漏。

union 后有 32 条保存的 P/Q 事实句柄改变 canonical key。这不是 32 个新
来源，也不能仅靠观察新的 producer 插入事件唤醒所有新匹配。

## 维护成本和解释边界

结果 JSON 同时记录索引扫描行数、Q 引用数、桶数和配对数。
当前实现每阶段重建索引，最后一阶段扫描 264 行、插入 132 个 Q 引用；
复合索引有 28 桶，来源索引有 36 桶（含无来源输入），另有 8 个已学配对。
这不是针对 union 的最优增量更新实现；不能用这些数字宣称某种索引的
维护耗时或峰值内存更优。候选数也不包含树索引内部比较次数。

本例的来源规则恰好编码了 Group，逻辑查询也显式要求 Group 相等，因此
来源能提供的过滤信息已经包含在合法的复合索引中。结果不排除复杂历史
关系或昂贵 residual 谓词有额外可复用信息，但否定了以下推断：
“训练时几乎所有匹配来自某些规则，所以以后只在这些来源中匹配即可”。

下一步应针对无法由已有连接键直接表达的重复子查询，测试完整有版本的
子查询结果缓存，而不是仅给来源标签换一种索引方式。

## 复现

```
CARGO_INCREMENTAL=0 cargo test --bin residual_reuse
CARGO_INCREMENTAL=0 cargo run --bin residual_reuse -- experiments/residual_reuse/results.json
```

无端到端加速或 e-graph 压缩率结论。所有代码与结果属于实验基础设施；
没有修改 egglog 内核，也不改变原始 baseline tag。

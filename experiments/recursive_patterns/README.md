# 共享 FractalComb 模板目录与观测 dominance

此目录同时包含线性 Extension 的 FractalComb，以及从实际依赖中提取的 `A -> 多个 B -> A` 模板。
所有定义都与具体实例分离。没有增加新的求和/累加 DSL，也没有做自动的全输入语义 dominance 证明。

## 使用

```sh
cargo build --release
EGG_LAYOUT_TEMPLATE_CATALOG=1 cargo run --release -- analyze --recapture-tier0 \
  --source experiments/recursive_patterns/dominance.egg --save-history --output out/catalog

EGG_LAYOUT_TEMPLATE_CATALOG=1 cargo run --release -- analyze \
  --replay-history out/catalog/history.json --output out/catalog-replay
```

输出兼容文件名 `recursive_patterns.json` 和 `recursive_patterns.html`。旧开关 `EGG_LAYOUT_DISCOVER_RECURSION=1` 仍可使用。默认关闭，避免普通分析产生完整诊断目录。

## 共享的内容

- `step_templates`：完整步骤定义去重。规则源码（含字面量）、输入角色、relative routes、别名约束都参与精确匹配。不同 Literal 与 Parameter 定义不被自动合并。
- `extent_templates`：展开范围描述去重。Depth 的含义还取决于引用它的模板，不能把共享 Depth 参数误认为展开图相同。
- `event_witnesses`：同一次实际 apply 的 binding、输出、行事实、union effect 只保存一份。实例通过 `event_refs` 引用它，`nodes` 仍保留每个分支位置及连接。
- 完全相同的模板定义合并目录项，原生来源 ID 保留在 `source_pattern_ids`。若同一 apply ID 对应不同证据，拒绝合并，不任选一个版本。

原生 egraph 的 `Extend`、`RefinedExtension`、`RecursivePattern`、extent constructor 本身继续使用 hashcons。目录共享补充的是序列化数据与跨模板实例证据，不能把 JSON 的字节变化称为 tier-0 enode 或运行时内存压缩。

相同数字或同一个 enode 可以被多个位置引用；共享证据不改写 nodes/ports 和 binding 数组，也不会因值相同而合并 apply 实例。

共享发生在单份历史的事件命名空间内；尚未实现跨独立历史的实例 ID 合并。

## dominance 与排序

实现的是**观测实例支配证书**：对被支配模板的每个实例，另一个模板必须有同一实际 entry event 的实例，保留其全部 apply，并且外部父依赖不更多；覆盖还必须严格增加。

这保证在本次历史中存在可核对的覆盖关系，不证明另一个模板在任意输入、资源状态或未来范围上都更强。因此：

- 不对非相同模板执行 union；`semantic_dominance` 明确为 `unproved`。
- 覆盖重叠但入口不同的模板保留，记录 `overlap_without_observed_dominance`。
- 不再仅因为一个出口集合包含另一个，就提前丢弃较小候选。
- 保留所有模板，先遵守 dominance 偏序，再贪心选择“尚未覆盖的唯一 apply 数 / 独立模板定义字节数”最高者。
- `new_apply_events` 去除排名靠前模板已覆盖的事件，累加不会重复计数。此排序是启发式，不是全局最优分割，也不是 enode 压缩率。

## 多出口范围及事实边界

`FractalComb` 的首参已统一为 `ExpansionExtent`：

```
FractalComb(Depth(n), pattern, start_ctx, binding)
FractalComb(SparseExtent(nodes), pattern, start_ctx, binding)
```

`RecursivePattern` 引用一个入口步骤及一组带 binding 转移的出口。Depth 仅在实际单元完整、均匀且没有共享节点被当成树复制时使用；稀疏 DAG 保留 OpenPort、ReturnFrontier、BoundaryPort 和 ExpandTo。
OpenPort 表示没有匹配到该模板出口的已验证 B apply，不证明 B enode 不存在或未来一定可执行。

启动/预热留在 start_ctx；重复主体入口及内部成员必须是实际闭合的 SmoothComb。当前只学习两类规则之间、每个 B 有一个已验证返回的单元；多父/多返回及未定位情况保守保留为边界。至少两个实际返回单元支持一个候选，不证明出口数量对所有输入封闭。

## 已验证结果

- 二分叉：1 个模板，2 个 Depth(4) 实例；每个实例 15 个单元、45 次实际 apply。
- 三分叉：1 个模板，3 个 Depth(3) 实例；每个实例 13 个单元、52 次实际 apply。
- 两个相同 B 创建点经 hashcons 去重后，只识别单出口，不冒充二分叉。
- 部分展开：每个实例 13 次实际 apply、8 个 OpenPort；不生成缺失 B 的事实。
- `dominance.egg`：保留 3 个目录项，找到 1 条观测支配关系。3 个共享步骤定义，34 份唯一事件证据；排名新增覆盖依次为 24、10、0。
- `deduplicated.egg` 的两个错位模板共享 8 个事件，但入口不同，因此没有支配证书。
- Math 6 轮目录包含已有线性 R23 模板：4 个实例，共 12 次唯一 apply；没有新识别出的多出口模板。

详见 `catalog_results.json`。测试包含精确去重、固定字面量/参数区分、不同入口、额外外部依赖、证据冲突、重叠覆盖不重复加分，以及实际 egglog 运行/历史重放。

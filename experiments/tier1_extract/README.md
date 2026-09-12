# 从 native tier-1 提取整条 tier-0 combined rule

输入是已保存的真实 Math 前 6 轮 tier-1 图：1,389 个 Comb e-class、1,529 个应用实例。
`extract.py` 为每个 e-class 选择一个实际存在、有限 unit-tree-cost 的表示，跨根共享父引用；
不枚举新候选，不调用旧 composer，也不连接 babble。

## 直接查看

- `index.html#comb_0033`：先看整条组合的 LHS/RHS，再看 tier-1 表达式及末端原规则。
- `combined.egg`：1,026 条整组合规则，包含中间构造器和 union effect。没有输入或执行日程。
- `combined.json`：逐 Comb 的导出结果、实际父依赖展开次序、实例映射、额外入口等式及未导出原因。
- `existing_combs.egg` / `extraction.json`：原有 tier-1 统一提取结果，未改变选择策略。
- `tier0_rules.egg`：17 条被引用的原规则字典，与 combined rules 分开保存。
- `../comb_order/ranked.egg`：兼容旧路径，内容与 `combined.egg` 相同。

例如 `comb_0033` 从父 DAG 展开为 R0 → R4 → R2 → R2：交换、减法展开、两次结合。
整条规则有 4 条 LHS 条件、12 条 RHS 动作，不再仅打印末端的 R2。

## 输出从哪里来

1. `native_templates.json` 保存真实 native Comb 构造器图；`extract.py` 只选择现有 e-node。
2. 原压缩快照丢掉了输出槽的表达式含义。`prepare_interfaces.py` 从同次导入的接口布局和读位置恢复这些元数据；不推导新连接，不使用具体 value 相等来合成规则。
3. `tier1_source_schema` 使用 egglog 自己的 parser 输出原规则 AST 和源位置。
4. `interface_program.py` 把元数据装入 `interfaces.egg`：`InterfaceLayout(Instance,String)`、`SourceRuleAST(RuleId,String)`，以及已有 `Occurrence` / `ParentAt`。
   **接口放在实例层，不改变 Comb 的 hashcons 键。** JSON 是接口载荷编码，不是新的匹配语言。
5. `tier1_interface_snapshot` 执行这份 native tier-1，校验全部 2,087 条 `LinkedParent`，从实际关系表读回 `native_interfaces.json`。
6. `lower.py` 只读取选中的 native 构造器及读回的接口，解释 `ParentPort` / `External`。
   同模板的不同实例分别实例化，共享的同一个祖先实例只执行一次。

这是对 tier-1 的解释/降级，并非声称 tier-1 之前已经存有完整 Math rewrite 字符串。
规则语义来自存入 tier-1 的源 AST；所有组合连接来自 tier-1 父引用和 relative binding。

## 语义边界

- 保留 SSA 构造和 union；不把后发生的 union 回写到已发出的 LHS，从而不会消掉早先 effect。
- 不从两个构造器的输出相等反推孩子相等。
- 必需的入口相等性成为显式 LHS guard；这些输出是带前提的组合实例泛化，不承诺涵盖某模板所有可能绑定。
- 1,141 个实例得到 1,026 条规则；同一 Comb 的不同接口/祖先共享形式分别检查，只有生成文本相同才归为一个 variant。
- 387 个实例遇到中间结果相关的 coarse join，保守报告 `needs_staged_matching`；1 个实例没有新 effect。
  这表示**当前降级器不能把它们表达为单条入口匹配规则**，不是证明不存在其他等价规则。
- 当前只支持此 Math 构造器集和 union 动作；不把原生算术函数当构造器，也不宣称任意 `.egg` 已适配。
- 本任务不改变原有提取/排序策略，不测加速或压缩率。

## 重现

固定输入足以重现 native 接口读取和降级，不需重新运行 tier-0：

```sh
cargo run --bin tier1_interface_snapshot
python3 experiments/tier1_extract/lower.py
python3 experiments/tier1_extract/render.py
python3 experiments/tier1_extract/validation_cases.py
cargo run --bin tier1_check_combined
python3 -m unittest discover -s experiments/tier1_extract -p 'test_*.py'
cargo test --test tier1_direct_extract
```

若从同次原始导入重新制作接口：

```sh
cargo run --bin tier1_source_schema -- experiments/annotated_export/math_microbenchmark/source.egg experiments/tier1_extract/source_schema.json
python3 experiments/tier1_extract/prepare_interfaces.py MANIFEST.json NATIVE_TIER1.json PROFILE.json
python3 experiments/tier1_extract/interface_program.py
```

`interfaces.egg` 是固定的、可复现的 native 输入。`interfaces.json` 和 `validation_cases.json` 是可再生中间文件，不跟踪。

## 验证含义

`validation.json`：1,026 / 1,026 通过原生 egglog 类型检查及有限边界种子上的源规则重放。
每例只运行父 DAG 指定的原规则日程，**不运行 combined rule**，然后检查其每一个 RHS 构造及 union 是否已由原规则产生。
种子满足所打印的 LHS（包含显式等式），原规则每步仍按 egglog 正常方式匹配；这不是一对一匹配次数或性能对照。
这些是有限回归证据，不是对所有输入的形式证明。
Python 语义测试另外检查双交换保留 effect、相同模板不同实例不被混淆、union 不回写 LHS 和 coarse 动态等式拒绝。

# Union 转移：先检查真实输入，再决定 Bake 什么

这次完成的是 standalone Cell CPU 的 union 工作负载调研和 Bake 请求导出。
**没有实现已认证的 RipSat Cell A → B 切换，没有新的性能提升结论。**
使用与 UNION_GRAFT.md 相同的 sum-bits Pass 3、SK/marker 编译产物和原调度；
没有跑 raytrace，没有新增候选预算，也没有重新选 marker。

## 先核对我们到底有什么 Cell

这份冻结的编译产物的 Analyze 记录仍是旧结果：89 个组合全部因
`Undefined sort CellStructuralKey` 被拒绝，注册的共享闭包为 **0**。
脚本现在自动报告这个库的状态、失败原因与文件摘要，不能再只看运行时实例数。

这不表示源码中的修复失效。仓库已经有两个独立的后续结果：

- `RIPEN_KEY_ENVIRONMENT.md`：结构键环境修复后 72 个组合饱和，17 个回放失败。
- `LOCAL_CLOSURE_SCOPE.md`：收缩依赖范围后，72 个饱和结果仍被真实相关的
  `ExtractedExpr` 自定义 merge 合约挡住，共享闭包认证仍为 0。

本次没有把那些独立结果冒充已接入当前编译产物。运行时仍能运行普通规则配方、
marker、已编译接续；这些不是通过认证的完整 RipSat 状态切换。
因此当前数据回答的是 **union 影响了哪些运行时配方实例，哪些变化值得送去 Bake**，
而不是“一个已经完备的 RipSat 转移库的命中率”。

## 统计口径

`CELL_CPU_UNION_IMPACT=1` 是默认关闭的诊断模式，不改变匹配、RHS 或调度。
它在每次有效 union 前记录受影响的存活配方实例，并在 repair 后采集结果。
按 `(Run epoch, instance)` 去重；一次实例可能接收多次 union，全部事件与顺序保留。

需要区分以下信息：

- **新建实例**与既有实例分开：本轮生成的 RHS 自身收尾，不算旧 Cell 升级。
- **直接 Apply union**与 congruence 派生 union 分开。后者是结果，不是预知输入。
- 统计 Bake 输入时，只使用本 Run 第一次派生 union 之前的直接 union 前缀。
  后续 Apply 若依赖已经完成的 repair，也不混进这个入口。
- 输入种类/数量、输入和输出的别名分组在第一次相关事件前截取；
  不从后续派生事件补填输入状态。构造计数按 canonical 参数键去重。
- “leaf input”仅指不是查询表结果列的参数，**不是已证明的 RipenInput 投影**。
- `input_external` 指另一端不等于该实例的查询输入或当前持有的构造结果，
  **并不证明它位于某个完整 RipSat 闭包之外**。
- repair 计数按事实当前所属实例归属，不把“关联了这些修复”说成“已经省掉这些修复”。

诊断扫描了 1,830,099 次实例，无未就绪输入；这是观测开销，不能拿它测速度。
本次 13,384 次 repair 访问全部被归属，没有重复归属或未归属访问。

## 分布

| 类型 | 实例×Run 受影响次数 | 不同实例 | 归属的 repair 访问 |
|---|---:|---:|---:|
| 原规则配方 | 6,954 | 2,396 | 10,391 |
| marker | 1,693 | 543 | 2,133 |
| 辅助规则配方 | 261 | 132 | 278 |
| 单构造存储实例 | 57 | 15 | 92 |
| 初始加载实例 | 35 | 1 | 490 |

原规则配方中，846 次是当轮新建实例。既有且已执行完配方的输入受影响记录有
**5,598 次**，其中 **3,308 次**有直接 union 输入，**2,290 次**只有同余传播。
有效 union 总数为 1,089：576 个 Apply 来源、513 个 congruence 来源。

3,308 次直接输入记录覆盖 **135 个创建模板**。前 10 个模板只覆盖 **35.4%**；
按“模板、参数位置、接入的构造种类”细分有 832 种，前 10 种占触及次数 21.7%。
这不支持“只 Bake 少数几个参数对，就能覆盖主要负载”的猜想。

| 直接输入变化 | 实例×Run 次数 |
|---|---:|
| 与实例外部结果合并 | 3,223 |
| 两个已有输入互相合并 | 18 |
| 输入与实例持有的构造结果合并 | 159 |

类别可重叠。只看 leaf input 的 2,160 次记录，三类分别是 2,076、18、69。
所以主要方向是 **输入获得外部结构**，不是仅枚举输入参数的相等分区。

## 哪类变化比较容易复现

这里比较的是：创建模板、当前输入/输出别名、输入的构造种类与数量、
有序直接 union 变化、静态调度位置相同，最终的别名/构造数量摘要是否相同。
数值先作为参数，不作为模板名字；原始 guard 语义没有被删除。

只允许用更早 Run 的结果；同一 Run 的样本全部评估完才加入历史。
下表的“复现”**不是可执行转移命中，不是完整图相同，也不是省去查询的次数**。

| 创建配方 | 直接输入影响 | 较早 Run 的同摘要复现 | 关联 repair |
|---|---:|---:|---:|
| `Smaller(TCPair, TCPair)` 的 `>` 分支，rule 703 | 79 | 50，63.3% | 158 |
| 更低代价更新 `ExtractedExpr`，rule 700 | 248 | 79，31.9% | 168 |
| 候选初始化，rule 699 | 136 | 39，28.7% | 133 |
| `Single(Get) ≡ SubTuple(...,1)`，rule 251 | 181 | 30，16.6% | 260 |
| `Get → TermGet`，rule 696 | 148 | 0 | 316 |
| `AddContext(Concat)`，rule 521 | 55 | 1 | 127 |

整个集合是 **307 / 3,308 = 9.28%**。把具体整数也纳入模板键则降到
182 / 3,308 = 5.50%。这支持保留数值参数和精确 guard，而非每个具体代价单独 Bake；
它不证明数值可以忽略，更不证明不同整数下完整 effect 相同。

仅用创建模板、粗略 union 形状和别名等信息，1,138 个键中有 446 个对应多个目标摘要。
加入当前输入构造数量和调度位置后，2,617 个键中仍有 **38 个**对应多个目标摘要。
**创建同一条规则的实例，不等于处在同一个 Cell 状态。**

一个真实反例：epoch 1185 的 rule 251 实例 1126 和 8915，输入摘要相同，
接入的另一端都有 `AddContext:1, Get:2`，触发规则同为 rule 516 的 Uop 上下文传播。
repair 后一个输入拥有 2 个 Get，另一个拥有 3 个 Get。没有参数接线，光看构造名字
和数量无法区分。`ambiguity-example.json` 保留这两条观测和原规则。

## 应该怎样 Bake

优先级同时看重复证据、待修复工作和语义难度，不仅按实例出现次数排序。

1. **候选比较与函数更新族。** 最先验证 rule 703 的条件等价转移，然后接上 699/700。
   保留候选 Term、全部 TCPair 证据、旧函数值、数值 guard 和 merge 产生的 Smaller。
   数值只参数化，不冻结结果、只留赢家或跳过相关 merge。真实阻塞是当前闭包证书
   不支持 `ExtractedExpr` 的合并契约，需要将其编译为受检查的转移程序。
2. **元组/索引输入细化族。** 对 Get、Single、SubTuple、Concat、Add-Gets 的接入，
   Bake 完整的参数接线和 offset/length/index 条件，而不是 `union(slot, 某个 Get)`。
   union provenance 给出原规则和接线证据，输入获得新结构后再规范化为目标 Cell。
   修复负载较多但复现较弱，需要扩大语义表示，不能直接扩大缓存键表。
3. **上下文、Drop 族。** 保留 ctx、scope、类型和 delayed union/subsume 的阶段约束。
   此次复现证据较弱，应在前两类有净收益之后再扩展；不能靠删掉上下文字段提高命中率。

通用注册键应该是：

```
已认证的当前闭包状态
＋ 相关边界槽位之间的完整接线
＋ 外部新增结构/等价证据
＋ 参数条件、merge 合约、允许的阶段
       → 目标闭包、身份投影、完整 effect、失效条件
```

先由真实 rule/merge 验证一次转移，运行时才能重用。只做创建-rule分类，或只观察
结果摘要重复，均不能授权“直接把 Cell 指针切到 B”。后续的性能验收要测净搜索、
净修复和端到端时间，扣除实例定位、守卫和失效维护的工作。

## 交付与验证

- 自动导出所有 **135 组、2,617 个观测形态**的 Bake 工作请求，无 top-K 截断。
  保留类型槽位、原 query/effects、触发规则、阶段、观测索引及不同目标。
  每一项明确 `ready_for_runtime=false`，不是伪装成证书的缓存记录。
- 紧凑统计、全部模板排名及请求目录位于 `union-bake-results/`。
- 8 项新测试通过：直接/派生区分、同 Run 防泄漏、关联/顺序保留、原状态区分、
  多目标歧义、旧报告拒绝、真实多层同余链与新旧实例分离。
- 原有 14 项 union-graft 集成测试通过（包含全部 52 个原有语义小用例）；5 项 Rust 单测通过。
- 同产物观测关闭/开启 **1,622 个快照及完整 binding 序列一致**，包括全部 marker 表、
  函数、等价关系、可见性和调度。不是与 pristine egglog 的新增全图等价声明。

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml --bin eggfuse-cell-runtime --example project_stages
EGGFUSE_TEST_FRONTEND=/path/to/eggfuse_frontend python3 -B -m unittest experiments.cell_runtime.union_bake_survey_test experiments.cell_runtime.union_graft_test -v
python3 -B -m experiments.cell_runtime.union_bake_survey --compiled out/union-graft-sum-bits-indexed/compile --output out/NEW --audit
```

原始观测：`out/union-bake-survey-final-sum-bits/diagnostic/runtime/report.json`。
最终摘要/请求：`out/union-bake-survey-verified-sum-bits/`。
`--report-only .../diagnostic/runtime/report.json` 可不重跑引擎，按相同已验证程序重新汇总；
没有完整的 per-touch 事件来源或程序不同会拒绝。
大报告、完整工作请求及构建产物不提交。观测开关保持默认关闭。

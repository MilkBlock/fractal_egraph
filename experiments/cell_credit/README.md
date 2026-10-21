# Credit 归给证明，而不是执行它的模块

Switch 空回答不领取 Cell credit；Get 的新端口读取是实际工作，搬到 CPU 后仍须计费。原先把“从 GPU 移除了多少查询”叫作 Cell 贡献，会奖励普通调度和后端迁移，不能用它选择 Cell。

本次实现了一个更严格、可运行的 **证明组反事实对照**。它测的是实际 raytrace Pass 3，未修改原规则和调度，也不是一个独立小 matcher。

## 对照保持什么，改变什么

两条路径都保留：

- 相同 CPU 参数化执行器和全部 31 个程序的固定根接线；
- 普通 scalar 查询接管、依赖订阅、端口缓存；
- 相同 GPU 查询计划、refutation 和后端位置；
- 原生 RHS、union、merge/rebuild 及原调度边界。

唯一开关为 `CELL_JOINT_PROOFS=0/1`。关闭时走正常 dataflow，既不运行 fresh-DAG/静止闭包宏，也不建立 uniform-cost 范围证明来退役候选或直接完成 Smaller 闭包。相应证明的运行期监测与状态维护也关闭。开启时恢复这些能力；其检查和维护全部计入计时。

这是 `terms-joint-closure-proofs` 证明组的条件净收益，不是整个 Cell 框架相对 pristine 的收益，也没有把任意 Analyze 的 Cell 自动转成这种证明。

## 单次配对结果

同一个 Rust 二进制、Swift worker、输入和调度。两组使用新 GPU 运行，复用已核验 CPU/完整图控制；不复用旧 GPU 时间作 credit。无计数器插桩，不取平均。

| 指标 | 仅关闭联合证明 | 开启联合证明 |
|---|---:|---:|
| 推理时间（包含运行期维护和原生 effects） | 16.474770 s | 15.195910 s |
| Setup（另列） | 2.928676 s | 3.013655 s |
| 普通 Cell 查询回答次数 | 30,566 | 30,566 |
| 普通查询返回 binding 数 | 1,918,606 | 1,918,606 |
| Get 新端口读取 | 977,455 | 977,455 |
| Get 缓存端口读取 | 0 | 0 |
| Switch 根评估／端口读取 | 0 / 0 | 0 / 0 |
| Switch 空回答请求 | 1,798 | 1,798 |
| If 新端口读取／缓存读取 | 20,035 / 32,701 | 20,035 / 32,701 |
| Fresh-DAG 宏实例 | 0 | 0 |
| 比较闭包消费次数 | 0 | 1,185 |
| 基于单调性停止重查的候选记录 | 0 | 73,607 |

观察到的组级 credit：**1.278860 s，推理时间减少 7.7625%**。这个结果包含所有公共查询原来就要支付的成本；并没有再给 Get、Switch 或 If 的普通查询接管分配 credit。这里只是一次固定输入的配对观测，不是所有任务的稳定加速保证。

最终全部事实、值、等价关系和可见性通过相同提交协议 CPU 的完整图门槛。既有 fused-native 差异仍单列，没有变更验证口径。未重新做 Tiger extraction。

## 为什么比之前的指标更合适

令 B 为共同执行能力，L 为其它已保留的结论，C 为待评价 Cell/证明组：

```text
credit(C | L) = J(B, L) - J(B, L + C)
```

J 是同口径实际成本。它已经包含维护开销，不应重复扣费。没有使用过的 Cell 结论不因命中、接管查询或空表就自动得正分；收益也允许为负。

- **Switch**：当前收益是普通空表短路，两边共同具备，不能奖励为 Cell 结构收益。
- **Get**：两边都实际读取 977,455 次。不能拿 GPU 访问减少量代替净收益，更不能将它与 CPU 读取次数硬相减。
- **比较闭包＋候选退役**：实际消费了范围有效的语义结论，且只关闭这些结论会增加运行成本，因此可以给这一组记 credit。

共享不能重复领功。本次 1.278860 s 只属于一个证明组，不能分别发给三个 Smaller 规则、上层 Cell 和等价 spine。单个 Cell 的独立 credit 尚未测。

## 用于后续 Cell 设计的机制

1. 固定公共基线，普通索引、空表短路和通用取值在两边都可用。
2. 记录**结论被消费**：哪个 Cell/证明、有效版本、替代了哪个查询或饱和义务、由谁实际采用。不能只统计匹配落在哪个 Cell。
3. 同一义务只登记一次；共享证明维护也只登记一次。等价 spine 使用同一个证明身份。
4. 用只撤去该结论/证明组的对照检查净收益。当前库改变后重新评估，不能假设 credit 固定可加。
5. 对两个可互相替代的 Cell，不可因完整库下两者删除边际都为零就同时删除；先按替代组处理，或逐个删除后重新评估。

当前已实现第 1 步及本证明组的第 4 步，并输出证明消费计数和共同执行成本。**一般的逐义务唯一归属、任意 Cell 的自动消融和逐 Cell credit 尚未实现。** 本次不新增全量历史流，也不声称旧计数足以恢复这些信息。

## 使用

两边都运行 bench skill 的 `run --all-rules --joint-sat`，分别设置 `CELL_JOINT_PROOFS=0` 和 `1`。保留同样的其它开关、worker 与缓存控制。

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py cell-credit \
  --baseline out/cell-credit-generic --candidate out/cell-credit-proofs
```

工具拒绝关闭整个 handler、换 GPU 版本、改变公共计划、启用插桩、图不一致或没有实际消费证明的对照。`cell-value` 已降为 GPU 工作迁移账，schema v2 的 `cell_specific_credit` 为未归因，不再将 `observed_*_delta_share` 当作 Cell 选择奖励。

[完整结果与两组指纹](results.json) · [统一 credit 协议](../../skills/cell-assisted-egglog-bench/references/credit.md)

验证：58 个相关 Rust 回归、34 个 bench skill 测试及两组完整 Pass 3 图检查通过。新回归覆盖证明关闭时保留 dataflow/端口、更新与 alias 后图一致，以及重新开启时不使用旧闭包证书。

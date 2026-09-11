# 符号 BindingMap 与公共构件：第一版

目的：将已观察的组合转为可以比较、分解和研究关系的端口程序。不是
matcher 加速实验，不改变 egglog 运行，也不把语法共享当成已证明宏等价。

## 输入与表示

输入为已有 math 前 5 轮的 124 个源位置组合类，582 个 consumer 实例、
834 条生产—消费连接。通过原生 AST 和函数 schema 提取变量类型与顺序，
用 in:N/out:N 代替实例名字。顺序来自各源规则的 LHS，不按运行时 Value
相等性合并变量。因此交换与复制能够被区分。

`materialized:Op(args)` 表示原生读写见证已经物化的值，不是在这个分析器
中重新运行 Op。验证分两部分：2606 次顶层变量值直接和原 bindings 比较；
730 次嵌套表达式值来自经过源位置确认的原生行参数。映射赋值、重复输出
约束再与 consumer binding 核对。本版本只接受 row 来源；涉及 rebuild
和变化的等价 epoch 时明确拒绝，需要先扩充版本化接口。

归一化只做：consumer 变量赋值定向、重复目的端口形成等价条件、精确重复
条件去重、等式两边按固定顺序排列。不会从 Add(a,b)=Add(c,d) 推出 a=c、
b=d，也不会分解任意构造器等式。27 个组合类仍有未赋值的 consumer
端口，它们需要额外的匹配见证，不伪装成确定性赋值程序。

原始 rule 的所有 LHS/head/guard 仍按 profile.rule_labels 引用保留。
BindingMap ID 只表示相同的归一化 wiring，不表示这些 rule 的全部条件
或 egraph 更新效果相等。当前没有完整条件逻辑化简或物化操作的通用组合。

## 构件学习与关系

- 先对 wiring 做精确去重：124 类得到 100 个端口关系。
- 对唯一关系中的子程序进行带类型的 anti-unification，重复 hole 必须
  匹配同一子程序，类型不匹配则拒绝。
- 每轮最多比较 20000 对候选，最多引入 8 个构件；按扣除定义成本后的
  AST 节点收益贪心选择。不是最优或穷尽搜索。
- 主实验拒绝 signature-only 构件，要求出现具体端口连接、重复值 hole
  或物化节点结构；另保留无过滤语法压缩对照。
- 每次替换后完整展开，断言所有唯一 BindingMap 与原树完全一致。
- 只对全赋值、无 residual、无物化节点的纯路由提供 compose_routes。
  通过端口代入建立 5 条组合关系，包括双交换恒等和复制后的交换不变。
  它们是 wiring 等式，不是已执行的新宏，不允许据此 union 图状态。

## 当前结果

主实验选出 8 个可展开构件。描述长度（AST 节点单位）从精确去重及引用的
2355 降为包含构件定义和引用的 1616。未去重的原映射为 2683 节点。
所有 582 个实例通过见证约束核对。不同构件用量见 math.md。

该成本只覆盖 binding 关系的表示，不包括完整 rule 合同、历史见证、
日志、图存储或 JSON 元数据，不是内存压缩率。无过滤对照可能压得更小，
但其中可以只是重复的记录结构；我们不以最大纯语法压缩作为研究结论。

尚未实现递归构件推断、归纳证明、带 materialization/guard 的完整变换
组合、未见数据的泛化验证或规则层 egraph。这一版验证的是：现有端口
关系能否被共享构件无损表示，以及一小部分路由是否有可证明的组合律。

## 复现

```
CARGO_INCREMENTAL=0 cargo test --lib binding_program
CARGO_INCREMENTAL=0 cargo test --bin binding_motifs
CARGO_INCREMENTAL=0 cargo run --release --bin binding_motifs -- experiments/annotated_export/math_microbenchmark/profile.json experiments/binding_program/math.json
python3 experiments/binding_program/render.py
```

测试覆盖重复 hole、类型拒绝、无损展开、非单射构造器等式、重复目的
端口约束、双交换恒等，以及篡改 consumer binding 后拒绝验证。

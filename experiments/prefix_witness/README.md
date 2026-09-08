# Prefix 消融 1：复用 rule apply 见证

本项只回答：**已发生规则匹配的局部，能否利用见证低成本获得结构 prefix 实例？** 尚未加入 Zobrist 历史、block 划分或快捷规则启用。

## Prefix 的定义

源规则是无条件的等式 rewrite：

```text
Add(Mul(x,y), Neg(x)) → Add(Neg(x), Mul(x,y))
```

识别三个结构 schema：

```text
深度 1：Add(p0,p1)
深度 2：Add(Mul(x,y),Neg(x))
深度 2：Add(Neg(x),Mul(x,y))
```

实例保存 `schema_id + canonical root + 有序边界绑定`。深度 2 的 LHS 叶边界按树位置为 `[x,y,x]`，RHS 为 `[x,x,y]`，不能丢掉参数位置和共享关系。同一个 e-class 可以有多个不同的 prefix 实例。

## 见证法

1. 使用真实内核 TraceSession 获取源规则的 Survived 逻辑匹配。
2. 读取保留的 x/y 绑定，并在 source 执行后 canonicalize。
3. 对 Mul、Neg 和两个 Add 做已有构造器的 keyed lookup，确认 LHS/RHS 结构在当前图中存在。
4. 直接构造 prefix 实例，不重新搜索其结构模式。

源规则匹配支持 LHS 的存在性；RHS 仍做执行后的查表确认。没有把 Survived 等同于“实际新生成节点”。测试包含 RHS 早已存在、source.updated=false 的情况。

## 对照

|模式|行为|
|---|---|
|source_only|只运行源规则，作为计时下限，不输出 prefix|
|global_rematch|源规则运行后，使用 egglog 原生查询在全图重新识别 prefix|
|rematch|预先给定活跃 Target(root)，使用原生查询只识别这些 roots 的 prefix|
|witness|复用源规则 trace 的绑定，通过查表构造 prefix|

`rematch` 是一个较强的对照：它预先知道精确的活跃 root 集合，不需要自己寻找位置。所有模式都在计时前准备相同的 Target 关系和检测规则。

原生检测通过带 trace 的冗余 union 规则取得绑定，并检查 `updated=false`。这是现有公开 API 的实现方式，包含 action/trace 开销，不是理想 query-only 接口的成本下界。

## 测量口径

- 计时包含 source 规则执行、witness 模式额外的 trace 成本、事件处理、RHS 确认查询及 prefix 结果分配。
- 不包含初始图构建、检测规则编译以及计时后的 oracle 验证。
- 内存指标是相对准备好图之后的阶段分配增量，不是 e-graph 压缩率。
- 每种配置在独立 release 进程中运行 5 次，共 100 次。具体时间和范围见 `results/prefix_witness/report.md` 与 `summary.json`。

## 正确性与覆盖边界

每次 rematch/witness/global_rematch 都与独立安装的新原生 oracle 查询精确比较 prefix 集合，避免使用已经耗尽 seminaive delta 的旧检测器做验证。

活跃输入生成 128 个根，共 512 个 prefix 实例。对这些受控局部，rematch 与 witness 集合完全一致。另加入三种情况：

- 有额外的 RHS-only 区域：全局查询能找到，source 见证不能自动覆盖。
- 有 4,096 个不触发源规则的 Add 根：它们仍满足浅层 prefix，但没有本次 apply 见证。
- LHS 和 RHS 预先已经等价：规则不修改数据库，见证仍然有效。

**即使在一个发生 apply 的 e-class 内，也可能存在该次见证没有覆盖的其他 prefix。** 独立测试预先 union 一个额外 Add 替代项，证明 witness 集合只覆盖其中一部分。这不妨碍用事件见证建立一个特定 prefix 的 block，但不能把这个 block 宣称为整个 class 的完整结构。

共有 5 项测试，包含参数位置／重复变量约束、局部精确一致、全局覆盖缺口、同 class 其他替代项，以及无有效 mutation 的见证。

## 实现边界

本原型限定为保留 x/y 的一个无条件、不删除节点、单 E sort 的规则。它不是通用物理 LHS row witness，也没有逐 mutation 归因。重建、merge、直接 action、新替代项和 block 生命周期需要另外的增量失效机制。

下一项可单独比较：相同事件和相同 prefix 实例上，显式规则历史与 Zobrist 历史摘要的更新、查询、内存及碰撞确认成本。集合、次数和顺序是不同的历史语义，不能用一个 XOR 结果混为一谈。

## 复现

```sh
cargo test --bin prefix_witness
cargo build --release --bin prefix_witness
python3 experiments/prefix_witness/run.py

# 128 个活跃根，4096 个不触发源规则的 Add 根
target/release/prefix_witness witness fresh 128 4096
target/release/prefix_witness rematch fresh 128 4096
```

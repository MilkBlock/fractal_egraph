# 分部积分自组合：1、2、4、8 层覆盖对照

这是明确手选的候选族，不是自动发现或已认证的无限 fractal rule。

```
R23: Integral(Mul(a,b),x)
  = Sub(Mul(a,Integral(b,x)),
        Integral(Mul(Diff(x,a),Integral(b,x)),x))

T(a,b,x) = (Diff(x,a), Integral(b,x), x)
```

每次只沿 RHS 的 residual Integral 进入下一次 R23。没有插入 commutation、
association 或其他 coarse rule。原始 .egg 的全部 11 轮完成后才开始测量，
分母仍是 1,047,896 个原始构造器节点。

## 三种入口

1. **固定 binding**：a=Cos(Var("x"))，b=x=Var("x")。这来自原程序已有的
   product-integral seed，不创建新 seed。
2. **固定入口 e-class**：上述 seed 在最终图中的 e-class，存在两个 R23
   LHS binding，包含运行中形成的另一种乘法排列。不是说这两种排列在
   原始运行开始前就都存在。
3. **全入口对照**：最终图全部 69,273 个 R23 LHS binding。此时任何后继
   binding 已在入口集合中，累计覆盖不应因重复访问它而增加。

每条转移必须保持同一个 x，并通过只读查找取得真实的 Diff(x,a) 和
Integral(b,x)。完整状态要求 LHS、RHS 及二者 union 等价关系都已经存在。
缺少 RHS 或等价关系时停止；从不生成假想节点。当前图有 12,548 个完整
状态，RHS probe 中没有缺 LHS 或缺 union 的实例。

## 结果

数字是去重后的 LHS∪RHS 覆盖节点：

| 深度 | 固定 binding | 固定入口 e-class | 全入口对照 |
|---|---:|---:|---:|
| 1 | 8 | 15 | 118,988 |
| 2 | 14 | 27 | 118,988 |
| 4 | 26 | 51 | 118,988 |
| 8 | 50 | 99 | 118,988 |

固定 binding 的 LHS/RHS 分项在深度 8 为 16/48，联合却只有 50：中间
LHS 已包含在前一步 RHS 中，不能相加成 64。固定入口 e-class 的分项
是 31/96，联合为 99。重复 binding 和不同入口的共享覆盖都只计一次。

深度 8 时固定 binding 和固定 e-class 仍分别有 1/2 个待检查后继，因此
本实验没有宣称八层就是终点。

全入口对照的 LHS/RHS 覆盖为 92,823/54,458，与此前单规则 probe 一致，
共同覆盖 118,988 个节点。对于这个仅由 R23 串联而成的候选族，这些递推
节点此前已经被全图单步 probe 统计过。它不能解释这份全入口统计的覆盖
缺口；但这不排除其他含 coarse 规则的族覆盖更多节点。

覆盖不增加也不等于族压缩没有价值：把多步内部连接封装后，可能减少
实例数或把原来的外部边界变成内部边界。本轮没有计算这种替换收益。

## 验证与边界

- 真实 egglog 执行和原生 LHS/RHS 标记查询；匹配通过当前表行映射到节点。
- 各层用节点集合求并集，canonical binding 重复时停止重复访问。
- 原始构造器表逐行检查未变化。
- 七项测试通过，覆盖实例重叠、同类不同节点、连续 binding、循环去重，
  以及小图中缺少 RHS／缺少 union 时不继续展开。
- “完整状态”只表示当前结构和等价条件成立，不是历史执行次数或来源证明。
- a、b、x 是接口；不会把它们代表的整个子图自动纳入覆盖。

## 复现

```
python3 experiments/integration_family/run.py
CARGO_INCREMENTAL=0 cargo test --bin integration_family
```

`results.json` 含分层 LHS/RHS 数量、新增覆盖、停止原因及 binding 转移样本。
[comparison.md](comparison.md) 提供简表与递推示意。

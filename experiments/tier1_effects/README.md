# Tier-1：显式 effect、smooth 判定与局部依赖冗余

这里的条件满足、效果传播和冗余关系由真实 egglog 程序 `ir.egg` 推导，
不是 Rust 中模拟一份 e-graph。Rust 适配器负责有类型的 BindingRelation 实例化、
构建候选以及验证待省去 producer 不在替代上下文的祖先中。

输入仍是调用方提供的正向契约与外部事实；尚未自动从 tier-0 trace 提取，
也未连接 babble。Ready 表示这个契约实例在 tier-1 中成立，不表示已执行 tier-0 mutation。

## IR

```
Ctx = Entry(id)
    | Join(Ctx, Ctx)
    | Apply(Ctx, rule_id, binding_key, required_effects, produced_effects)

Effect = HasFact(predicate, typed_arguments)
       | Equal(typed_value, typed_value)
```

Smooth/coarse 是推导状态：Apply 保留相同身份，前提补齐后增加 Ready/Smooth 事实。
缺少外部 binding 的 Apply 不获得 Grounded，也不会提供 ProducedEffects。
补齐 partial binding 时创建新的具体候选，原待定表达式保留。

HasNode 编码为保留 sort、operator 和子参数的 HasFact；不会由构造器输出相等反向推导子参数相等。
不存在“未绑定输入默认作为新输出节点”的语义。输出 effect 也检查端口范围、sort 和 groundness。

## 原生 tier-1 关系

- `Provides(ctx,effect)`：可用 effect；Apply 的前提成立前不传播其输出。
- `EqAt(ctx,a,b)`：上下文局部等价及对称/传递闭包，不 union 全局 Value。
- `ArgsEqual` / `Satisfies` / `AllSatisfied`：按同一组已实例化 binding 联合检查所有前提。
- `DominatesForUse(provider,apply)`：provider 满足这个具体 Apply 的全部前提。
- `AlternativeSupport(old,new)`：同规则、binding、require/produce 契约的新候选已经 Ready。
- `RedundantForUse(old,new,removed)`：替代候选成立，且其上下文独立于 removed。

DominatesForUse 是效果满足关系，不是控制流路径意义的 dominance，也不是 Ctx 等价。
缺失前提不作为单调负事实写入：未推导 Ready 只表示尚无证书，后续可补齐。

所有要求共享同一份 binding 字典。例如 P(x) 与 Q(x,y) 不能分别选两个不相等的 x。
当前没有自动枚举未绑定的存在变量；需要调用方给出完整 witness。

## 如何安全省去依赖

当前只产生从直接 Join 父节点中省去一支的替代 Apply：

```
old = Apply(Join(A,B), R, binding, requires, produces)
new = Apply(A,         R, binding, requires, produces)
```

保留二者，让 native tier-1 验证 new 的前提。
适配器对原上下文 DAG 做祖先检查：如果 A 间接依赖 B，就不给出 Independent(A,B)。
不会使用“经过 B 后才存在的效果”证明可以绕过 B。

证书只针对这个 use：old 可能仍带有 B 的其他输出，所以不 union old/new、不删除 B。
若未来要替换整个上下文，必须另外证明完整外部接口与可观察 effect 保持。
本阶段没有实现全局提取器、通用 dominance 最优化或给 babble 的等式生成。

Entry 是外部公理根；不能把来自 removed 的派生事实伪装成独立 Entry。
尚未校验这些公理对应的 tier-0 committed 证据，这是后续 trace 适配器的职责。

## 动态更新

`add_entry_effects` 只接受 Entry 上的新增 ground effect。输入加入后，再运行 tier-1
saturation，原 Apply 的 Ready/Smooth/DominatesForUse 自动更新。
依赖图与旧组合不被修改。回滚、删除或公理撤回不在本阶段语义内，需要新建分析实例。
等价 effect 只影响拥有它的上下文及后继，不泄漏到其他 Entry/分支。

## 验证结果

5 个语义测试通过：

1. P(a)、Q(b,y) 缺 a=b 时不产生 T(y)，加入上下文 equality 后成立，原无 equality 的分支仍不成立。
2. Apply 只需要 P 时可省去额外提供 Q 的支撑；old 仍有 Q、new 没有，因此不错误合并上下文。
3. 替代路径间接依赖被省去的 producer 时，即使替代 Apply Ready，也不能获得冗余证书。
4. 未绑定 y 时不制造 T(y)；到达的新 Q 能使同一个已绑定 Apply 从 pending 变为 smooth。
5. 构造器输出相等不反向统一子参数，拒绝跨 sort equality 和未声明输出端口。

`results.json` 来自测试中的原生查询，包含 union、动态更新前后以及冗余/拒绝冗余候选。

## 复现

```
python3 experiments/tier1_effects/run.py
cargo test --test tier1_effects --test trigger_bridge
```

API 位于 `src/tier1_effects.rs`，原生规则位于本目录 `ir.egg`。
这是一份小规模语义基础设施，不是 Math 的压缩率或运行时内存收益证明。
当前 EqAt 使用有限域上的闭包，尚未优化大型上下文的共享与增量支撑索引。

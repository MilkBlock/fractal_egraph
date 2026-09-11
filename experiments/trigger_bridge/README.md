# BindingRelation 与 trigger 桥接

这一阶段没有研究交换律，也不假设 fractal 算子正交。

## 实现

`src/trigger_bridge.rs` 提供三个接口：

- `BindingRelation`：typed input/output ports、正向事实条件、等式约束；其余
  出现的变量是局部变量。`then` 将后一段变量移入独立命名空间，以等式连接
  前段出口和后段入口，保留中间变量以及重复端口，避免笛卡尔积枚举。
- `gap`：对给定图摘要和部分 binding 检查 trigger。已知等价关系可满足
  关系参数的要求；缺少事实、缺少等式和未绑定变量分别保留。不会从
  `F(a)=F(b)` 推断 `a=b`，也不会为了启用 trigger 直接创造缺失事实。
- `search`：在调用方提供的有限 ground 正向动作中搜索桥接方案。每一步
  检查前提后才能累积其效果。报告保存模板键、计划、最终状态契约和剩余
  缺口。缺条件的动作不执行，预算耗尽是 unknown；搜索失败只针对供应的
  动作集合。外部变量尚未绑定时报告 binding_unresolved，不猜测 witness。

`canonical_key` 按接口和条件出现顺序重新编号变量，因此变量编号不同的
关系可共享模板；常量、类型、重复变量约束会保留。它不规范化条件重排，
不执行一般的关系等价判定，也没有对构造器实现完整 congruence 推理。

组合后的关系保留存在变量，当前 gap 求值要求调用方提供这些变量的具体
witness；尚无 join 求解器自动枚举或索引外部候选。桥接只处理显式 ground
contracts，拒绝隐含的 entry/exit 焦点路由。它尚未与 effect_orbit 的周期
候选自动联接，也没有从 .egg AST 或 combine 历史自动提取动作摘要。

## 可复现结果

测试模板要求 `Ready(x,y)`，候选桥接规则是：

```
Seed(x,y) -> Link(x,y)
Link(x,y) -> Ready(x,y)
```

| 起点事实 | 具体 binding | 桥接长度 | 原生 egglog check |
|---|---|---:|---|
| Ready | (10,20) | 0 | 通过 |
| Link | (11,21) | 1 | 通过 |
| Seed | (12,22) | 2 | 通过 |

三例使用同一个关系模板，但桥接长度不同。这是人为构造的小用例，验证
表达能力，不是实际 math workload 的复用率或内存压缩率。

另一个原生用例确认：已有 `Ready(A,B)`，在 union B/C 后可满足
`Ready(A,C)`。其他检查覆盖中间变量、重复端口、变量重命名、类型错误、
缺前提、未绑定变量、预算边界和禁止构造器反向分解。

搜索本身是独立符号原型；测试另外创建真实 egglog 实例执行所选方案并
检查结果，不使用逻辑 match 数代替已提交效果。没有修改内核或优化调度，
不支持删除、否定、自定义 merge、I/O 或未建模的执行效果。动作契约是
调用方提供的前提，不是由该模块认证的原生规则语义。

## 复现

```
TRIGGER_BRIDGE_REPORT=experiments/trigger_bridge/results.json CARGO_INCREMENTAL=0 cargo test --test trigger_bridge
```

结果 JSON 包含初始缺口、共享模板键、所选桥接及最终状态契约。

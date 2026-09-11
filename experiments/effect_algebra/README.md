# 带效果的 BindingMap：固定位置的第一版

本轮实现 Summary，而不是仅给纯路由追加文字说明。它保存：

- entry/exit：具名焦点及选中节点的符号语法，包含 typed input ports；
- requires：需要存在的节点/关系事实；
- adds：新增节点/关系事实；
- equalities：新增 union 关系。

Summary::then 做同时替换，将后一段的接口接到前一段的 exit 上，检查
类型、重复端口与选中节点形状，合并正向效果；已由前段产生的事实不会
再次成为外部前提。缺少显式绑定或需要额外匹配时返回错误。匹配对象是
已选择的节点描述，不从 e-class 相等推断其孩子相等。

归一化用集合吸收重复事实，去掉入口已保证的添加；等价关系按连通分量
保存为规范星形边，而不是展开所有点对。这个方法是保守的：没有对任意
嵌套事实做完整 congruence 商化。相同规范形是等价的充分条件，不同规范
形本身不是不等价证明。负例另外用原生执行构造了不同结果。

binding_route 可将固定形状焦点的参数变化投影回已有 BindingMap。联合
比较保留图效果和焦点，same_graph_effect 则明确忘掉焦点。absorbs 和
absorbs_graph 对应这两种观察，不能混用。

## 结果

单个固定 Add 位置得到三个状态 Id、S、E，其中 E=S²：

```
Id --S--> S --S--> E --S--> S ...
S³=S, S⁴=S², E²=E
```

完整的 3×3 组合表自动计算，全部 27 个结合律三元组合检查通过。三状态
闭包给出所有 n 的递推表示；这是在当前符号模型中的有限闭包验证，没有
声称为 Agda/SMT 证明，也没有由有限采样直接推断任意规则规律。

两个独立位置得到 9 个联合状态、18 条生成元转移。除了 0–8 次单位置
交换，还在真实 egglog 上验证了长度不超过 4 的全部 31 种左右交换序列。
每一步都核对节点和关系行数、各个事实以及表示的等价划分。

反例包含：混用不同位置、Seen(x)→Seen(Step(x)) 持续增长。闭包搜索达到
8 状态预算时返回未闭合，不把预算耗尽当作不动点。

## 适用边界

原生验证仅覆盖预先声明的 swap/grow 模板；还没有从全部 124 个历史组合
自动抽取效果。当前没有接入内核调度、替换实际 rule，也没有测量内存
压缩或速度收益。多个位置的独立性由固定接口与这里的正向操作保证，
不是对任意不同 AST 路径一概声称可交换。

忽略时间戳、provenance 多重性和 match 计数；如需保留历史，重复次数应
作为额外参数存储。删除、I/O、自定义 merge、非单调 guard 和未建模的
交错执行不在当前模型中。对缺少事实蕴含证明的组合，可能保留更强的
外部前提；不能声称计算了所有可能状态上的最弱前置条件。

## 复现

```
CARGO_INCREMENTAL=0 cargo test --lib effect_program
CARGO_INCREMENTAL=0 cargo test --bin effect_algebra
CARGO_INCREMENTAL=0 cargo run --release --bin effect_algebra -- experiments/effect_algebra/results.json
python3 experiments/effect_algebra/render.py
```

algebra.md 是可读状态表和原生结果，results.json 保存完整摘要。

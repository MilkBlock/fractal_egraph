# RipSat cell 演化：先测结构重复，不替换执行

本实验直接观察实际 egglog 运行中的 match、完整 LHS 读取和已提交的实例通知。
不修改规则，不开启 Join/RHS 剪枝，也不把观察到的模式作为执行证明。
普通运行默认不记录这些数据。

## 结果

固定库、单线程、observer 开/关两组独立执行；图判等检查每个构造器与
canonical eclass 的双射，不仅比较节点数量。完整结果见 `results.json`。

| 工作负载 | 新增且有边界关联的 cell 实例 | 转移模式 | 在不同实例间重复的模式 |
|---|---:|---:|---:|
| 12 次边界扩展 | 12 | 1 | 1 |
| 12 次 RipenInput 具体化 | 12 | 1 | 1 |
| 6 次双 cell 接合 | 6 | 2 | 2 |
| 同一输入上的无关新 cell | 0 | 0 | 0 |
| math microbenchmark，6 轮 | 2,394 | 3,211 | 2,124 |

math 中有 3,380 个逻辑 match，67,732 次**锚定视角**记录。一个 match 可能
接触多个重叠 cell，所以这个大数不能当成消除了多少匹配。
去重后，2,631 个有记录的边界 match 中，2,606 个参与了跨实例重复的模式。
这仍然只是观察覆盖率，不是可以安全跳过的匹配比例。

按“来源族—规则—目标族”只有 90 种连接。补上读取形态后为 1,618 种，
补上目标 binding、别名变化及提交结果后为 3,211 种。
其中 **675 种读取形态出现了多个结果变体**，最多 26 个。
因此已有重复结构证据，但还不能把读取形态直接编译成确定性状态机。
已有目标事实、同轮其他 union，以及观察接口缺失的上下文，都可能影响结果。

math 的 observer 开/关结果精确一致：2,167 个 e-node、748 个 e-class。
这里的两组都使用同一份固定 Bake 库；不是与裸 egglog 做速度比较。

## 一个具体例子

规则：

```egg
(rewrite (A x) (B x))
(rewrite (Pair (B x) (Flag x)) (Done x))
```

旧主体 `T` 已经提供 `A(x) = B(x)`。之后加入 `Pair(B(x), Flag(x))`，
第二条规则产生一个 `U` 实例，其中 `Pair(B(x), Flag(x)) = Done(x)`。
`U` 复用了 `T` 的 `B(x)`，并接入外部 `Flag(x)`。
12 个不同实际 x 的这段过程归成了同一个模式；原来的 T 并未被消耗或复制。
训练使用 99，观察样例使用 0…11。

输入细化例子使用 `B(C(x)) -> Done(x)`。旧 `T` 的输入后来被 union 到
`C(y)`，观察器会记录具体化条件、相对槽位、目标 binding。输入即使与
主体输出 alias，仍保留输入角色。相关回归测试专门覆盖了这一点。

双主体接合保留参与 cell 的 ID、族及共享槽位映射。统计采用逐个来源
cell 锚定的投影，因此六次双主体接合有十二个锚定记录、两个模式，
**不是发现了十二个独立接合，也未证明两个完整超图等价**。

## 哪些信息可以进入转移

1. 旧 cell 来自实际已提交的 `adaptive_instance_*` 通知；通知不等于新 e-node。
2. match 必须具有完整物理 LHS witness。
3. 来源必须提供被读取的实际 body fact，或者一个被读取构造器的输出正是
   来源的 RipenInput。仅共享一个参数值不够。
4. 目标 cell 必须由同一 match 的已提交实例通知登记。不会按“同一轮出现”连边。
5. 参数按来源槽位、外部读取符号和目标本地符号编码，保留重复值的别名关系
   与实际 primitive 常量。`new` 只表示编码中的新本地符号，不表示新分配节点。
6. 提交的 Math 行优先引用目标模板的行号，不重复抄写整个主体；模板外的
   residual fact 才显式编码。提交结果会按步末 canonicalization 归一化。
7. union 的已知读取值连接与前后别名分区分别记录。原生 union 总数和 trace
   能归因的 union 数分开报告，不能把 trace 中的零当成没有 union。

模板身份保留原规则与精确槽位结构；当前不尝试跨规则的图同构判等。
每个模式只保留一份展示样例与聚合计数，原始事件在每轮后释放。
这是诊断模式，仍有成本，不是已完成的加速运行时。

## 一个被否决的做法

第一版把所有共享 Math 值的接触都展开，math6 得到 894,287 条锚定记录、
14,567 种模式；其中大部分只是松散接口接触。负对照保存在
`broad_contact_pilot.json`，不作为最终复用证据。

最终版不按预算、Top-k 或频率删候选，而是用上述读取/输入具体化定义
排除了 826,555 次松散接触，不为它们构造转移。

## 边界

- 当前接入正构造器 Math adaptive runtime，不声称支持任意 `.egg` 程序。
- 未产生宏实例通知的 source-only 效果没有被归类为一个新 cell。
- 一轮中刚出生的 cell 不倒过来当作该轮 read-only match 的父因。
- input-refinement 标记说明匹配读取了输入的具体结构；它本身不能证明这个
  构造器刚刚出现，也不能证明它是唯一原因。
- 隐式 union 的完整因果链还不完备；步末别名变化可能同时受其他规则影响。
- 不做最小 guard 求解，不从有限重复推出无限规律，不进行自动快捷执行。
- DOT/SVG 是族之间的**锚定投影图**，不是完备或确定性的状态机。

## 复现

```sh
cargo test --offline --test reuse_evolution
cargo build --release --offline --example adaptive_math
RAYON_NUM_THREADS=1 python3 tools/bench/cell_evolution.py \
  --output out/my-cell-evolution --rounds 6
```

输出包含各组源码/库摘要、精确图判等、汇总 `report.json`，以及每个
`*-observe/` 下的 `cell-evolution.json`、`transitions.dot` 和 `transitions.svg`
（SVG 需要已安装 Graphviz）。脚本中的外部 30 秒/4 GiB 保护仅用于诊断进程，
不是候选发现策略。

单独开启观察：

```sh
EGG_LAYOUT_CELL_EVOLUTION=1 cargo run --release --offline --example adaptive_math -- \
  frozen SOURCE.egg out/my-observation LIBRARY.json
```

下一步需要判断哪些结果差异仅是“目标已存在”，可以表达为统一的
`ensure Use(U, binding)`；哪些差异需要补充真实的状态条件。
不能仅凭本次频率统计就把 native Join 替换掉。

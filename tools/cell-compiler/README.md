# Cell CPU 编译与 bench

在仓库根目录用 Cargo 编译。锁文件包含 Rust marker 求解器 microlp。源码变换、原生 Analyze/Bake、整数规划、Recipe lowering 和计时均由 Rust 执行。训练时使用本仓库的原生 egglog。计时对象是独立 Cell CPU 执行器。

## 从源码生成程序

```sh
cargo cell-compile --case sum-bits --output out/cell-sum
cargo cell-compile --source my.egg --bake-source training.egg --output out/cell-custom
```

`--output` 需要一个新目录。对自定义输入可省略 `--bake-source`，直接用结构键变换后的执行程序训练。另给训练输入时，编译器先核对训练中使用的类型、merge 定义和规则。预置 case 包括 `fact`、`loopfact`、`sum-bits`、`dot-product`、`sum-divisors`、`fenwick_tree`、`jacobi-1d` 和 `raytrace`。它们按清单拼接并校验 SHA-256，共用仓库内冻结的小训练输入。四个新增 Pass 3 的来源、规模和运行命令见[较大负载对照](../../experiments/profile_suite/README.md)。[套件清单](../../experiments/profile_suite/suite.json)

输出包括原始源码、`source_sk.egg`、Bake 观察目录、`source_sk_mk.egg`、marker 选择和证明、Recipe、`program.json`、各 CPU 计划与文件摘要。结构键只响应源码显式的 `cell-choice/v1`。marker 只选取符合条件的构造子片段，保留原规则的效果，在相关 Run 前安排完整性规则。Bake 记录可匹配的观测 Cell。源码采用现有 CPU 后端支持的扁平输入，初始化之后可使用 schedule 和 check。

默认 marker 目标是最小化结构维护成本减去省下的查询原子数，在最优解中再最大化消费位置数。等价目标可有多个最优布局。可传 `--profile FILE` 使用 `marker-cost-profile/v1` 中的实测费用。输入的 SK 源码摘要必须匹配，`marker-selection.json` 的 `offered_fragments` 提供片段 key 和消费规则名。`--marker-locality all` 可复现实验中的标量连接策略，默认值为 `identity`。`--composition-evidence FILE` 接收绑定到该 Recipe 摘要的规则对，生成独立验证的多规则续接计划。

## 运行同程序对照

```sh
cargo cell-bench --program out/cell-sum/program.json --lane baseline --repeat 5
cargo cell-bench --program out/cell-sum/program.json --lane nodes --repeat 5
cargo cell-bench --program out/cell-sum/program.json --lane filtered --repeat 5 --verify-stages
```

每个 lane 在独立子进程中运行，清除继承的 `CELL_CPU_*` 实验开关。三者采用默认 merge programs 和普通存储。`nodes` 开启编译后的共享入口，`filtered` 还开启 Sieve 和列交集。每次重复都重新构图。JSON 中的 `median_ms` 只计执行 schedule 的时间。编译、构图、导出及逐阶段审计都在计时之外。

`counts.all_candidate_visits` 汇总直接候选、残余候选、merge 候选行、Spine pivot 候选和共享入口谓词检查。输出同时记录这五项、程序与最终状态的摘要。`--verify-stages` 在单独进程中对比当前 lane 与基线的初始图、每个 Run 的可见性和绑定及调度字段。首次试跑也可以让 bench 在临时目录内编译。

```sh
cargo cell-bench --case sum-bits --lane filtered
cargo test --release --locked --manifest-path tools/cell-compiler/Cargo.toml
```

在入口分发前过滤改动之前的五次重复中，冻结的原始 sum-bits 输入生成 1,428 个 Run。三个 lane 的候选检查依次为 73,400、20,437 和 17,028。中位推理时间依次为 96.28、86.20 和 76.51 ms。基线和过滤版逐阶段结果一致。

另行用带结构键声明的研究版 sum-bits 对照，旧 Python loader 接受 Rust 产物，旧、新编译器生成的 marked 程序在原生 egglog 上投影后 3,468 行、2,324 个等价类一致。用同一检查器比较 SK 和 marked，两个编译器都在 `TermBop` 遇到原有差异。独立 Cell CPU 与 marked 原生端点也有一条 `TermBop` 对不上。上述 bench 只用来比较 Cell 内部策略。[已有端点对照](../../experiments/cell_runtime/SOURCE_NATIVE_BASELINE.md)

原生 `egglog-baseline` 在同一 marked 输入上的 Free Join 枚举和探测合计为 75,174 次，向 action buffer 交付 9,114 个绑定。[对照数据、计数定义和复现命令](benchmarks/free-join/README.md)

开启共享入口、Sieve 和列交集后，调度器在构造消费者 seed 前检查 Sieve。如果某个必需的连接摘要已经构建且为空，当前查询可以直接返回空结果。写入发布之后，下一个 Run 重新判断。bench 的 `consumer_seed_deliveries` 统计实际交付的 seed，`sieve_probes` 统计 Bloom 探测，`empty_join_skips` 统计由空连接摘要排除的规则调用。三个计数单独列出。

冻结 sum-bits 上，相对同一 filtered 配置的旧实现，seed 投递从 58,905 降到 32,792，Bloom 探测从 32,258 降到 22,909，原有口径的候选检查从 17,028 降到 16,779。残余候选仍是 9,309 次。初始图及全部 1,428 个 Run 的图、绑定和调度通过逐阶段审计。

对于只含构造和复制的 RHS，普通存储路径先在当前图上按原顺序查找构造。在第一次查找未命中时登记 Cell 并创建对应事实。如果所有构造都已存在，运行时只保留该次执行的编号消耗，后续事实的等价类编号与原路径一致。`counts.cells_created` 是登记到图中的 Cell 数量，`counts.rhs_frames_avoided` 是免去登记的 RHS 次数。

实验 lane `free-join` 在 `filtered` 的同一份索引上按共享列探测残余关系，`--verify-stages` 直接与 `filtered` 比较。候选访问与新增的键分组、探测分开计数。[八个 Pass 3 对照](benchmarks/free-join/CELL.md)

`free-join-entry` 会在分发共享 seed 前按完整 LHS 探测相连的表。编译时加 `--emit-free-join-entry` 可将计划写进程序，旧产物在选择这个 lane 时从 typed Recipe 推导相同的计划。[入口计划与八组对照](benchmarks/free-join/ENTRY.md)

`free-join-planned` 复用目标表版本内的投影结果，并在单列连接中按实际增量规模选择是否从小键域反向枚举。可以用同一产物和 `free-join-entry` 配对。[Raytrace 与 sum-bits 的入口探测、原生规划及计时](benchmarks/free-join/PLANNER.md)

`cargo cell-bench --lane free-join-planned --modules union-graft,stage-spine` 在同一份产物上切换已有运行时模块。`--verify-stages` 将开启模块的执行与同 lane 关闭模块的执行逐阶段对照。局部同余可用 `--modules local-congruence` 请求，并查看输出中的实际启用状态。[两组 Pass 3 消融及局部同余阳性对照](benchmarks/runtime-modules/README.md)

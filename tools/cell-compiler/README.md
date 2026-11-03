# Cell CPU 编译与 bench

在仓库根目录用 Cargo 编译。锁文件包含 Rust marker 求解器 microlp。源码变换、原生 Analyze/Bake、整数规划、Recipe lowering 和计时均由 Rust 执行。训练时使用本仓库的原生 egglog。计时对象是独立 Cell CPU 执行器。

## 从源码生成程序

```sh
cargo cell-compile --case sum-bits --output out/cell-sum
cargo cell-compile --source my.egg --bake-source training.egg --output out/cell-custom
```

`--output` 需要一个新目录。对自定义输入可省略 `--bake-source`，直接用结构键变换后的执行程序训练。另给训练输入时，编译器先核对训练中使用的类型、merge 定义和规则。预置 case 支持 `fact`、`loopfact`、`sum-bits`、`raytrace`，按套件清单拼接并校验 SHA-256，共用仓库内冻结的小训练输入。[套件清单](../../experiments/profile_suite/suite.json)

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

在本次本机五次重复中，冻结的原始 sum-bits 输入生成 1,428 个 Run。三个 lane 的候选检查依次为 73,400、20,437 和 17,028。中位推理时间依次为 96.28、86.20 和 76.51 ms。基线和过滤版逐阶段结果一致。

另行用带结构键声明的研究版 sum-bits 对照，旧 Python loader 接受 Rust 产物，旧、新编译器生成的 marked 程序在原生 egglog 上投影后 3,468 行、2,324 个等价类一致。用同一检查器比较 SK 和 marked，两个编译器都在 `TermBop` 遇到原有差异。独立 Cell CPU 与 marked 原生端点也有一条 `TermBop` 对不上。上述 bench 只用来比较 Cell 内部策略。[已有端点对照](../../experiments/cell_runtime/SOURCE_NATIVE_BASELINE.md)

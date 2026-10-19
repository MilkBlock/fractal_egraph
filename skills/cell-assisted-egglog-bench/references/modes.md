# 模式与现有实现

- `run` → `examples/raytrace_stable_bench.rs`，默认稳定选择；`--policy original` 加 `--original`。保存 `report.json`、`setup.json`、完整 `graphs/{native,cpu,gpu}.json`。同提交协议整图失败会退出非零，仍保留导出供提取核对。
- `extract RUN` → `experiments/raytrace_pass3_gpu/extract.py RUN`，可加 `--reference-output OLD/extraction/native.egg`。Tiger 的生产 reader 要 pretty JSON；现有脚本已经处理，不能直接给它一行紧凑 JSON。输出 `extraction/{timing,quality}.json`。该目录已存在时不要覆盖，用 `report` 查看，或明确安排新的提取目录。
- `audit` → `raytrace_pass3_bench ... audit`，原策略 GPU 路径的独立 native-frontier oracle。没有新计时结论。`--three-lane-check` 改为原策略 `check`，附完整三路径整图核对，可能因已知原策略分歧返回非零。
- `cpu-pair` → `raytrace_pass3_bench ... cpu-pair`，原策略 CPU 自对照，检查两次完整图的一致性，不对两次计时求平均。
- `diff` → `raytrace_pass3_diff`。`--inspect /0/1/r1/12` 对已找到的边界进行回放；`--inspect final` 只做最终检查；`--open PATH` 展开指定 Saturate；`--sequential` CPU 先跑完再跑 GPU。`--sort-hoist` 是旧的“按 raw ID 排序”诊断反例，不是稳定选择修复；不要默认使用。
- `sat-skip` → `sat_skip_bench WORKER OUT N MODE`。默认 512 个实例/time；`--checked` 默认 32 个/check。它检查实际原生事件交接和每个公开调度边界。
- `parameterized` → `parameterized_cell OUT`，不启动 GPU。采用真实 Bop 和六条 terms helper 规则，对不同 Term/代价、缺失端口、union 和多候选别名运行同一份固定读取程序。每阶段必须同时通过融合原生整图、同提交协议整图、逐规则匹配计数和增量表镜像核对。没有具体闭包目录，也不表示整套 terms 已获 sat-skip 认证；见 `experiments/parameterized_cell/README.md`。
- `test` → shared-backend 的 `hybrid`、`table_live`、`stable_choice`，结构 hash 冲突测试，以及根包 `whole_cell`、`whole_cell_automatic`、`sat_skip` 和 Pass-3 比较器测试。需要实际 matrix worker，不能把缺环境变量后提前返回的测试当成 GPU 通过。

## 判读

`same_commit_verification.exact_rows_values_equivalence_visibility=true` 才通过同提交协议整图门槛。`verification` 是原生融合路径与 GPU 的另外一个门槛。两者不能混用；融合路径可能多物化了一些 Smaller/TCPair 中间事实，必须保留这一差别。

`gpu.audit=true` 表示诊断运行，不能拿来报性能。`gpu.runtime.skipped_saturations=0` 就没有 sat-skip 收益。报告提取质量用 `quality.passed` 和选定 DAG 检查，不靠树展开大小；共享 DAG 的树展开可以指数级大。

稳定策略的私有 key 行是额外存储。公开整图比较不会把这些辅助字段当成原程序事实；原程序的 `to-hoist` 始终在严格比较内。不要把隐藏辅助字段说成压缩收益。

历史与协议说明：`experiments/raytrace_pass3_gpu/STABLE_CHOICE.md`、`FIRST_DIVERGENCE.md`、`experiments/sat_skip/README.md`。这些文档是证据和口径，不是保证未来加速比的常数。先读脚本生成的当前结果，无须每次重读所有旧实验。

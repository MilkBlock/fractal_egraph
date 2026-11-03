# 复现实验

评估对象为 `a5e3e9ac61230347b0d5425bc0f62325fb290558` 的 Cell CPU 和 `ae80f82a403c1209fb084b115ee89470c7e66d0e` 的 eggcc。实验脚本位于本目录，生产路径没有新增实验开关。原始本机输出位于 `out/parallel-review`，版本化的 [results.json](results.json) 含逐次数据与分母。所有命令从评估工作树根目录运行。以下输出目录需为新目录。

## eggcc

使用已配置的 LLVM 18、CBC、zstd、生产 Cargo 缓存和 macOS APFS。schedule profiler 的 driver.patch 来自既有 eggcc-pass-benchmark 的 saturate-build 输出，SHA-256 为 `c20a544002b1dce465703187480e21e138acb784de1ff088d0b19ff71559418d`。本次运行所用 skill 配套 checkout 是 `cell-assisted-egglog`，其调度 runner 支持 `--timing-only`。eggcc 的 rust-toolchain 为 1.88.0。脚本在隔离 clone 中应用补丁，使用 COW 构建缓存，不写共享生产 checkout 或 Cargo 依赖源码。

```sh
review=experiments/cell_runtime/parallel_review
raw="$PWD/out/new-parallel-review"
python3 "$review/eggcc_matrix.py" "$raw" \
  /Users/mineralsteins/Repos/egg_related/check_statewalk \
  /Users/mineralsteins/.codex/skills/eggcc-pass-benchmark/scripts/bench.py \
  /Users/mineralsteins/.codex/worktrees/cell-assisted-egglog/egg_layout/out/eggcc-saturate-profiler-v2/driver.patch
```

计时应用使用 `optimized-pretty-print --eggcc-schedule parallel`（前者是 run-mode 的值）。profile 组使用现有 skill 的 saturate-run，运行三 pass 连续提取链。pool 组仅改线程选择与断言，记录 CLI monotonic wall 和进程的已等待子进程 CPU 账。两组的 Tiger 哈希在本机一致。线程选择补丁同时支持 1、4、16，进程记录实际池宽。原始 CLI、输出程序、每个 pass 的完整输入及时间包保存在各 lane。

## Cell 消融和审计

准备 [共享节点实验](../SHARED_NODE_SCHEDULE.md) 的已验证编译产物及 release 二进制，使用同一份 `out/shared-node-final/results.json`。脚本校验 program 和 binary 指纹，复用原有 timer、完整阶段比较器及存储 oracle。进入干净 shell，清除外部 `CELL_CPU_*`。四个区组的顺序平衡每个 lane 的序位。核心矩阵和两个残余过滤矩阵比较完整语义，物理存储矩阵的 oracle 额外对导出行排序。任何差异均保留失败。

```sh
ref=out/shared-node-final/results.json
mod=experiments.cell_runtime.parallel_review.cell_matrix
python3 -B -m "$mod" "$ref" "$raw/cell-matrix"
python3 -B -m "$mod" "$ref" "$raw/join-matrix" --join-policy
python3 -B -m "$mod" "$ref" "$raw/storage-final" --storage
python3 -B -m "$mod" "$ref" "$raw/plain-join" --join-policy --plain-storage
```

原始存储探索的第一条 lane 保存在 `storage-matrix`。它遇到导出顺序差异，未计入正式四路矩阵。正式 `storage-final` 按包含重复行的完整行序列排序比较。额外深度采集单独运行，保留原始计时；结果汇总在各层的 reads、candidate_rows、deepest_seed 和 fixed-rule 曲线中。

## 工作量剖面和片段回放

```sh
python3 -B -m experiments.cell_runtime.parallel_review.cell_probes "$raw" "$ref"
python3 -B "$review/summarize.py" "$raw" "$raw/results.json"
```

`cell-span.patch` 只对隔离副本启用已有的 query profiler，保存每个 Run 中各条规则的耗时及绑定数。span 模型采用对应计时矩阵 block 1 的未插桩阶段时间，按该 Run 各规则诊断时长的相对权重分配。它将搜索阶段准备/封包时间也乐观地分配给 worker，同时假设只读工作可以独立进行。结果给出零开销的可分割上限及不可分规则的调度估计。

`freeze-entries.patch` 另外采集实际执行的纯参数投影以及 oracle，和上一步分开出数。回放使用 Rayon 1.11.0、rayon-core 1.13.0 的常驻池。每个结果都逐值比较，包括正常拒绝。两个有序样本组在同一机器上分别预热两次、测十次，保留原样本。该回放是入口内核级诊断。端到端 Cell 并行收益仍需未来的在线实现验证。

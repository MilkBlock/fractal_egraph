# 仅在缺工具／需要重编译时读取

## Schedule-only profiler

先用 skill 的 `saturate-check`。已保存的生产调度 profiler 通常在版本化 checkout 的
`out/eggcc-saturate-profiler-v2/eggcc-schedule-profile`，旁边的 `provenance.json`
包含二进制和 driver 指纹。也接受 `--profile-binary` 指定其它已核验构建。

缺失时执行 `scripts/bench.py saturate-build --output out/新profiler目录`；随后
`saturate-run --profile-binary out/新profiler目录/eggcc-schedule-profile --output out/新结果`。
这调用现有 `build_profile.py --schedule-only`，只临时插桩/恢复生产 pass driver，
不会运行 coverage，不要求冻结 library，也不清理/修改原生引擎依赖。
构建之外的生产源码修改仍须保留，不并发执行其它生产构建。

## 完整时间 / 覆盖率 profiler

默认在 egg_layout 根目录操作，全部离线。已验证的生产依赖是 egglog ebba7bb、egglog-experimental 08771f9；当前脚本针对这套 API。

## 缓存路径

- profiler：`out/eggcc-pass-audit/eggcc-profile`
- 宏展开工具：`../check_statewalk/target/release/flatten`
- 覆盖分析：`target/release/examples/ripsat_coverage`
- 冻结库：`out/ripsat-coverage-remeasure-merged/train/library.json`
- 旧原始数据：`out/eggcc-pass-audit/{fact,loopfact}/`
- 可直接读取的表：`experiments/eggcc_passes/results.json`

## 只构建缺少的工具

```sh
python3 experiments/eggcc_passes/build_profile.py
LIBRARY_PATH=/opt/homebrew/opt/cbc/lib CARGO_TARGET_DIR="$PWD/../check_statewalk/target" cargo build --offline --release --manifest-path tools/eggcc_term_validate/Cargo.toml --bin flatten
cargo build --offline --release --example ripsat_coverage
```

profiler 构建脚本临时修改生产源码和缓存依赖，加计时点，复制独立二进制后恢复源码及原执行文件。它会清理两个相关生产依赖的构建缓存；不要并发对同一 checkout 构建或编辑。保留所有无关修改。脚本会检查修改过的文件是否被别人再次改动，不覆盖并发修改。

已经解决过的构建坑，不要重新试错：

- LLVM：`LLVM_SYS_180_PREFIX=/opt/homebrew/opt/llvm@18`。
- 链接：`LIBRARY_PATH=/opt/homebrew/opt/cbc/lib:/opt/homebrew/opt/zstd/lib:/opt/homebrew/lib`。
- 两项由 build_profile.py 设置；不要安装新 LLVM 或联网拉依赖。
- Cargo 对 Git checkout 的缓存可能不感知临时源码修改，所以该脚本有针对性的 clean。
- 原始 .egg 的 `unstable-fresh!` 等宏必须先经生产 experimental parser 展开；不能直接扔给覆盖工具。
- 展开后的内部 `@Name` 需要合法化；flatten 保持 `___Name` 命名以兼容旧冻结库，字符串内容不修改。

如果脚本因 API 漂移失败，保留具体错误并修现有脚本，不反复重编译或用另一版本的引擎顶替。不要通过改代码删规则来让测量通过。

## 冻结库缺失

先寻找用户已有的 library.json，并核对 version、patterns、signatures 和来源。不要偷偷换成 43 条模式后仍称为 163 条库。163 条库的再生成流程见 `experiments/ripsat_coverage/README.md`，需要旧的训练观察文件。

若数据缺失，先交付可测的计时并明确覆盖率缺失，或在用户请求重新 bake 后再构建新库；不能为生成表格擅自启动昂贵的全量 analyze。

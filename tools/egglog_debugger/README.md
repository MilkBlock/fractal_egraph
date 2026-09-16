# egglog-demo 原生调试桥

在 `egg_layout` 根目录启动（需要 Python 3.10+、Rust、`typst`、Graphviz `dot`）：

```sh
python3 tools/egglog_debugger/server.py --port 8080
```

打开 `http://127.0.0.1:8080`。默认使用相邻的 `../egglog-demo`；可通过
`--demo PATH` 指定。服务会构建本地 release 二进制，优先提供 demo 的
`static/` 源文件，WASM 等构建产物从该项目现有的 `dist/` 提供。
如果还没有 demo 构建产物，先在 demo 目录执行 `make`。

- 点击编辑器中 `rule` / `rewrite` 的任一行，预览整个规则的 Typst 公式或 DOT。
  使用原生 AST 源位置，支持多行、Unicode 和未加 `@pattern` 注释的规则。
- 点击 **运行并识别** 执行本目录 patched egglog 的实际运行时。
  可先粘贴 `experiments/bake/increment-3.egg`：6 个有效应用、5 个组合、4 条 Fractal 证据。
- 日志窗口按有效应用、Rule Compose、Fractal 过滤；点击每一行读取该事件的公式快照、
  binding、effects、父应用和路径证据。事件 ID 是实际 trace ID，不是连续的数组下标。
- **导出日志 / 回放日志** 保存和载入源码及公式；更改编辑器不会改写旧快照。
  **载入日志源码** 恢复运行时的编辑器内容。预览窗口可滚动，公式保持原始大小。
- **停止** 会取消请求并终止对应本地进程；默认每次运行限时 120 秒，
  可用 `--run-timeout SECONDS` 修改。失败和取消后保留已收到的部分日志。
- 原 demo 的 **Run** 仍运行上游 WASM；**运行并识别** 才使用本地 instrumented runtime。
  不把 WASM 的输出当成本地 trace 的结果。

## 语义与边界

这测量的是 **实际 egglog runtime**，不是独立匹配模拟器。collector 只向分析层导入有
实际 effect 证据、且具备完整物理见证的应用；`logical_matches` 独立显示，不能理解为
提交的 mutation 数量。一个 application 也不等同于一个 mutation。

tier-1、tier-2 保留同一 EGraph 和导入游标，只导入新应用和新 extension，
在每个简单 `run` 轮次后重新饱和增量关系，并在下一轮前发送 JSONL。
复杂 schedule 维持其原始语义，当前在采集结束时交付数据。
重复的稳定相对 binding 路径必须得到原生 `FractalComb / Represents` 的见证。
这些是有限已观察路径，不是任意次数递推的证明。

继承主分析器的输入范围：执行分析要求单个显式、自包含 `Math` datatype，暂不接受
`include`。源码预览没有该 datatype 限制，但不支持 subsuming rewrite。
组合能够合法降级时显示单条 combined rule；否则显示包含中间效果的完整分阶段 DAG，
并显示不能扁平化的原因。不会把它误报成一条可执行的等价 rewrite。

回放文件是已解析的调试快照，可重现相应公式，不是重新执行 tier-0 的原始 trace。
CLI 也可独立使用：

```sh
cargo run -- debug-patterns tests/fixtures/cli_math.egg
cargo run -- debug-stream experiments/bake/increment-3.egg
cargo test --test native_debug --test native_history --test native_single_process --test fractal_comb
```

浏览器回归（安装 Python `playwright` 及 Chromium 后）：

```sh
python3 tools/egglog_debugger/test_browser.py --url http://127.0.0.1:8080
```

本地 HTTP 服务只监听 loopback。代码、日志和公式通过同源 API 处理；不依赖外部公式渲染服务。
原 demo 页面使用的第三方前端 CDN 仍需联网。

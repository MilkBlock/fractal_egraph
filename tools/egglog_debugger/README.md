# egglog-demo 本地原生调试桥

在仓库根目录启动：

```sh
python3 tools/egglog_debugger/server.py --port 8080
```

打开 `http://127.0.0.1:8080`。默认使用相邻的 `../egglog-demo`（可用 `--demo PATH` 指定）；提供其 static 源码与已有 dist 资源。需要本地 Rust、Python、Node、Graphviz、Typst 和 Eggplant Pattern Preview 插件。不发布 gh-pages。

## 当前分析

“运行并识别”调用原生 egglog，逐轮显示真实应用、Coarse/Smooth layers、CS 组合和 saturated rule composition。Ripen 默认执行；相同入口共享执行，不同任务轮流推进。没有自动推理超时或候选上限，停止按钮通过断开流取消后台进程组。

Layer 面板可以载入 `analyze --output` 产生的目录。共享族支持来源 rule comb、CSCS/CCSS 引用、Coarse/Smooth layers、闭包图、合并前后原生 body。Top100 按不同 CS layer 覆盖数排名，累计覆盖去重。运行中尚无完成共享族时显示队列进度。

weak dominate 面板列出原生匹配证据、再入观察与无法判定原因。再入不是无限/耗散型规则的证明。旧 Use/模板/Fractal 推断入口已移除，页面不会用旧启发式结果替代新流程。

CLI、输出文件和当前语义边界参见 [主 README](../../README.md)。可用有限示例：

```sh
cargo run --release --offline -- analyze \
  --source tests/fixtures/core_flow/reentry.egg --output out/reentry-view
```

## 规则渲染

点击源代码中的 rule/rewrite 行，预览 Typst / DOT。预览复用 Eggplant VS Code 插件的 extractor、注解、MathView 与渲染管线。`@egg-viz-json` 标签/字段属于显示元数据，不改变 egglog 语义。页面保留 Typst / DOT 源码、binding/effect 证据和 Rust 转译视图。

服务中的预览/渲染请求超时与旧运行目录保留数量仅管理 UI 请求和磁盘，不参与 CS 发现、ripen 终止或判等。

## 检查

```sh
python3 tools/egglog_debugger/test_stream_errors.py
node tools/egglog_debugger/test_ripen_coverage.mjs
# 需要本地 Playwright 与 Chrome，以及已运行的服务
node tools/egglog_debugger/test_core_flow.cjs out/reentry-view
```

浏览器测试隔离外部请求，测试真实 layer 面板与本地 API；它不声称验证未缓存的外部页面依赖。

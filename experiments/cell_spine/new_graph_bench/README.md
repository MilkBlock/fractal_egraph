# 用已 Bake 的 Cell 库构建新图

复用现有基础设施：

- `tools/bench/math_virtual.py::measure`：独立进程、macOS `/usr/bin/time -l` 时间/RSS。
- `tools/bench/math_graph_equal.py::verify`：全部 Math 行和双射 e-class 对照。
- `examples/native_math_baseline.rs`：原生基线，现在新增 `bench MANIFEST [GRAPH_JSON]` 分段模式。
- `tools/cell_spine/compiler.py`：当前含父 Summary 接管的 symbolic 编译器。
- `out/native-compare/bin/pristine`：通过 `experiments/native_compare/results/final.json`
  中 SHA256 和 immutable `egglog-baseline` tag 校验的历史二进制。

旧的 `experiments/cell_machine/bench.py`、`tools/bench/math_virtual.py` 主程序和
`experiments/compact_cells/bench.py` 分别测试旧状态机、虚拟表、pack 存储，不能
代表本次的 generated spine rules。这里复用它们的测量和判等组件。

## 复现

在仓库根目录：

```sh
out/cell-spine-venv/bin/python tools/bench/cell_spine.py \
  --output out/cell-new-graph-bench-next --copies 1 1000
```

已构建当前 native 工具时加 `--skip-build`。默认每种情况只测一次，不取平均。
如果本地没有可校验的 pristine 二进制，加 `--no-pristine`，仍有同一当前内核的
native 对照。旧基线的构建入口是 `tools/bench/build_native_compare.py`。
此 benchmark 的图导出使用专门的 Math 格式，不是任意 sort 的 serializer。

每种算术形式只从一个训练输入 Bake 一次。随后产生 1 组和 1000 组全新
`heldout-i/x,y,z` 变量绑定，将它们放入一张全新的 egraph。没有对测试输入
重新做 analyze/Bake。库文件哈希在所有执行结束后必须保持不变。

1000 组测试每次都得到 **13,000 个等价 Math 节点、7,000 个 e-class**，并对
所有行和等价关系进行完整验证。pristine 与当前原生执行也逐图对照。四条规则
只含加乘交换、分配律及提取公因式；独立实例之间没有新的交叉操作，所以这
不是完整 math-microbenchmark 或复杂跨 Cell 边界的泛化证明。

## 本次结果

详见 `measurements.md`、`results.json`。下面是 1000 组新绑定的引擎时间：

| 变体 | 因式输入 ms | 展开输入 ms |
|---|---:|---:|
| 当前裸 egglog | 47.279 | 53.035 |
| Cell 紧凑图 | 38.607 | 73.730 |
| Cell 后完整导出 | 51.544 | 89.975 |
| pristine 参考 | 43.233 | 51.219 |

前端从已有 typed AST 生成 forked 输入，还分别需要 **8.797 / 12.434 ms**。
加上该阶段，Cell 紧凑图分别约 **47.404 / 86.164 ms**；完整导出约
**60.341 / 102.409 ms**。训练、Gurobi 和训练验证另需约 **0.240 / 0.270 s**。
构建二进制不计入任何执行时间。

因式形式的推理本身从 **10.401→1.239 ms**，但输入解析/插入仍需约 35 ms，
前端转换也抵消了紧凑执行优势。展开形式的推理从 **7.902→15.664 ms**，库
解析安装需 **13.288 ms**；当前继承规则路径没有带来速度优势。

紧凑图的进程峰值 RSS：因式 **10.11 MiB**（当前裸引擎 12.14）；展开
**13.84 MiB**（裸引擎 13.56）。完整导出时分别为 **11.53 / 16.95 MiB**。
库文本分别 **4,038 / 44,170 字节**；库的运行期解析结构已包含在 RSS 内。
文本字节不等于库在内存中的占用，不能拿两者相减。

因此本次只证明：当前库能够直接构造新绑定的正确紧凑图；在这一小规则集上，
生成完整新图仍未比裸 egglog 更快。不要将纯推理阶段的改善当作端到端加速。

## 测量边界

- 每个变体使用新的原生进程/EGraph，避免混用两种运行的峰值 RSS。
- 分段记录创建/安装、输入解析插入、规则运行、显式导出、文件读取、析构。
  run 仍可能含 egglog 的延迟准备，不等于纯 Join 时间。
- 计时/RSS 进程不扫描和序列化整个图；验证在额外进程执行。
- `spine` 不展开完整图；`spine_export` 生成真实 Math 表。二者的语义一致性
  由同输入的额外导出验证确认，不靠单纯节点计数。
- Cell 前端计时从已有 AST 开始；共同的合成工作负载生成不计入任何模式。
  用户输入若还需额外 AST 解析，那部分成本也不能忽略。
- 主对照是同一新编译 runner 的当前内核。pristine 是历史已验哈希 runner，
  用整份源文件执行而非分段调用，是附加参考，不能用于严格隔离内核差异。
- 30 秒/4 GiB 是复用的外部测量保护，不截断推理后宣称完成。所有报告的运行
  均正常结束并通过完整图对照。

回归测试：

```sh
out/cell-spine-venv/bin/python -m unittest tools.bench.test_cell_spine -v
```

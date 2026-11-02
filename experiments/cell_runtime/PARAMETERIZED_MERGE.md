# 参数化 merge 配方：可执行更新，不是终态替换

实现于 analyzer 的通用 Recipe IR 编译和 standalone Cell CPU。
不按 ExtractedExpr/Smaller/TCPair/marker 名称特判，不修改 Metal。
从 2026-11-03 起按用户要求默认启用；`CELL_CPU_MERGE_PROGRAMS=0` 显式关闭。

## 执行路径

`eggfuse/analyzer/merge_programs.py` 把真实 merge 表达式变成带类型的 SSA：

```
slot 0 = 本次提交真正读到的 old
slot 1 = 本次待写入的 new
后续 slot = 构造、精确 primitive、函数读取
return = 合并结果引用
```

`(Smaller old new)` 编译成构造，绝不偷换为 min。嵌套读取保留顺序，不做跨更新的
CSE；相同输入是否调用回调沿用原合并契约。缺失读取与整数溢出保持原失败行为。
同一键的连续 set 分别读取提交时的旧值，不共用最初的 old。

消费 merge 构造的规则被编译成固定的键/构造结果端口读取与 guard 程序，保留全部
候选、完整关联 binding 和原 RHS。没有完整接线的条目列在 unsupported，保留原查询。
纯字段投影的 Unit 函数发布继续传递上下文，所以真实 marked source 可以走：

```
ExtractedExpr 冲突 → Smaller → CellSpineView10 → 三条比较/union 规则
```

投影根据 typed query/effects 识别，不依赖名字。端口读取复用现有精确索引，不扫描
全部 eclass 入边，不新建失效缓存。union、新候选和 subsume 使用当前 canonical
可见行及原 delta 分区。完整 binding 进入现有 StagePatch，整轮匹配结束才提交 RHS。
没有提前执行下一阶段。Rust 独立重建类型、指令、投影与控制结构，拒绝伪造配方。

Analyze 发布 `parameterized-merge-programs.json`；CPU device pass 发布
`merge-programs.json` 并嵌入 `program.json`。两者共用编译模块，指纹和 manifest
覆盖新文件。新进程只加载文件，不在线调用 Python/Bake。旧文件在开关关闭时可用，
启用功能却缺少配方时要求重新生成 device 产物，不静默走空路径。

## 修复的两处实际问题

1. Native Analyze 的 typed core 原先遗漏结构键声明。源程序、准备好的 Cell 和
   Cell 重载导出现在都携带原 ABI、构造定义、字符串标签。重新 Analyze 同一小输入，
   未修改旧失败记录。72 个实例共享一份环境，其中有 23 个 merge 程序和 3 个比较入口。
2. CPU 在 union(hi,lo) 后曾把两侧引用都标为新行，即使 lo 一侧字段没变。
   这会重新触发不变的 PotentialExtractedExpr，重复写默认候选并增加 Smaller。
   现在只修复引用被替换 hi 的行，再归并两侧依赖。真实冲突仍完整处理。

第二项是共同执行路径的正确性修复，不能归功于编译配方。修复前 72 个隔离 Cell
只有 37 个与原生整图一致；修复后 **72/72 一致**。

## 验证结果

原生 oracle 通过 `cell_recipe --device-oracle` 从新 Analyze 库重放生成，不读取 CPU 结果。

| 检查 | 结果 |
|---|---:|
| 新进程加载真实隔离 Cell | 72/72 |
| 开关前后相同阶段执行 | 1,246 |
| 与原生完整事实/函数/等价/可见性一致 | 72/72 |
| 编译 merge 调用 | 141 |
| 编译消费者完整 binding | 246 |

测试覆盖多阶段构造及后来子代价更新、默认候选、多写冲突、中间节点、多候选 alias、
迟到输入、subsume、同价左侧选择、10^16 附近精确整数、嵌套读取、相同输入算术 merge、
失败行为、暂停恢复、marker 接续与伪造文件；也对照全部既有 52 个语义样例。
结构键导出、原生配方和依赖范围的 Rust 检查通过。

## sum-bits Pass 3 单次计时

同一 source→SK→marker 产物，只重新生成 CPU device pass。历史 Analyze receipt
中的失败不冒充成功静态 Cell；通用程序来自已验证的原规则/merge IR，新 Analyze 库
另行验证。所有构建和其他验证结束后，同二进制、单线程、每种配置只测一次：

| 配置（都包含 union 修复） | 推理时间 |
|---|---:|
| 原 merge 求值/匹配 | 142.161 ms |
| 编译 merge＋marker 消费者 | 139.108 ms |

减少 **2.15%**，只是单次小幅收益；后续按用户要求改为默认启用，仍保留关闭对照。实际执行 2,140 次 merge、
2,006 条内部指令；固定入口提供 413 个完整 binding，其中 marker 207、比较规则
48/152/6。端口读取 829 次、候选行访问 825 次、primitive 检查 412 次，均非免费。

两边 **1,511 个快照和完整 binding 序列一致**，逻辑 binding 都为 12,930，repair
访问都为 2,378。旧实验 repair 为 13,384；与旧版本约 0.22–0.24 秒的差距不能全算
编译配方收益，也不是同二进制加速比。中间 relay 版本的计时与 native 构建重叠，
不作为性能依据；最终计时单独执行。

**完整 Pass 3 对原生整图仍未通过。** 同一 marked source SHA 的历史原生快照也为
4,518 行，但精确比较器报告一条 TermBop 接线不能对应。没有用行数相等代替整图验证，
没有宣称 extraction 质量已验证，也没有重跑原生速度基线或 raytrace。

## 权限边界与产物

这是可执行参数化更新程序，不是静态终态替换。`terminal_cell_substitution=false`；
native 的 min/max 共享闭包门槛未删除。89 个组合仍有 72 个饱和、17 个回放失败，
静态共享闭包认证仍为 0。任意全局 Cell A→B 的免搜索替换仍需边界与转移认证。

原始结果：`out/parameterized-merge-native/`、`out/parameterized-merge-library-verified/`、
`out/parameterized-merge-final-sum-bits/`。精简结果和真实指令例子在
`parameterized-merge-results/`，不提交大快照、编译目录或二进制。

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml --bin eggfuse-cell-runtime --example project_stages
EGGFUSE_TEST_FRONTEND=/path/to/eggfuse_frontend python3 -B -m unittest experiments.cell_runtime.merge_program_test -v
python3 -B -m experiments.cell_runtime.merge_program_bench --pipeline out/union-graft-sum-bits-indexed/compile --output out/NEW --audit
```

后续引擎检查用 `--compiled out/NEW/compile` 复用编译结果。隔离库检查使用
`merge_program_library_check --analysis DIR --oracle FILE --output NEW`，运行前后核对二进制未变。

# GPU 后端通用空表短路

默认开启，**不依赖 Cell、不需要 Bake、不产生 Cell credit**。这补齐了原生 Free Join 已有的基础短路能力；没有修改原规则或 egglog 内核。

## 执行位置与正确性

编译器为每个查询导出必需的正向 LHS 表 ID。Swift GPU worker 已维护各表可见行数，现在在处理本批**全部原生增量**后、生成 GPU 查询起点前，剔除任一必需表为空的查询。

- 这是 GPU 后端的批次派发优化，不让 GPU 为每个候选重复查同一个空表。
- 判断使用批末状态。同批删除后重插不会被中间的零行数错误剪掉。
- 每批重新判断；新增事实、union/rebuild、subsume/删除后的恢复都能重新启用查询。
- 零 atom 规则没有必需表，仍可产生初始单位 binding；它不同于依赖一个空的零参数 relation。
- 即使整个查询批次都为空，也继续提交表/索引维护，并确认全部原请求的增量游标。
- query 列表压缩后保持原序；普通 Run 的冻结输入与原生 RHS 提交顺序不变。
- Rust 的独立表镜像会核验 worker 声称为空的查询确有空表，并拒绝被剪查询返回 binding。

此处只处理整个必需表没有可见行；非空表之间连接不上，仍由普通 Join、约束与其他优化处理。

## 真实 raytrace Pass 3 对照

两边均 **关闭全部 Cell 服务**，原始 547 条规则走 GPU 查询，RHS/merge/rebuild 由原生执行。稳定选择辅助规则仍是公共 CPU 适配器。相同二进制、worker、输入、调度和其他优化，只改变 `EGGLOG_GPU_EMPTY_TABLE_PRUNING=0/1`。每组只跑一次，未取平均；未变的 CPU/原生完整图控制从历史复用。

| 指标 | 关闭空表短路 | 开启 |
|---|---:|---:|
| 多 atom 查询起点 | 40,587,639 | 32,410,383 |
| 内层活候选行访问 | 72,750,548 | 66,967,939 |
| worker 剪掉的查询请求 | 0 | 10,272 |
| Switch 查询起点 | 2,243,450 | 0 |
| Cons 查询起点 | 1,121,725 | 0 |
| 无插桩推理时间 | 18.456533 s | 18.056397 s |
| 无插桩 GPU 设备总时间 | 6.147014 s | 5.790781 s |
| Setup（另列） | 4.358073 s | 2.703655 s |
| 最终报告的已分配 GPU 缓冲字节 | 747,262,352 | 478,826,872 |

起点减少 **20.15%**，内层候选访问减少 **7.95%**；本次单次配对的推理耗时减少 **2.17%**，设备总时间减少约 **5.80%**。存储数字是 GPU 后端缓冲的已分配容量，不是峰值 RSS，也不是 egraph 节点压缩率。

403 条规则至少有一次请求被短路。Switch/Cons 各有 1,665 次实际发送到 worker 的请求被剪掉；另外 133 次本来就在 host 的无增量检查中跳过。请求数不是 kernel 启动数，更不能乘表行数代替实测的起点差值。

计数由单独的 `CELL_SEARCH_COUNTERS=1` 诊断获得，不拿其秒数当性能结果。无插桩时的原生增量二进制上传仍为 184,444,160 bytes，binding 二进制下载仍为 75,558,224 bytes；这些字段不包含 JSON 元数据。最终完整图与逐规则 binding 产出数量相同。

## 验证

- 四个新计数/计时运行均通过同提交协议 CPU 的完整事实、值、等价关系及可见性检查。既有 fused-native 差异未被改写为通过；未重跑 Tiger。
- 491,386 次原生增量查询 oracle 检查全部通过；该 audit 无 Cell 服务，不能与开启 Cell 的旧查询次数混作同一工作量。
- 默认开启下 14 个 GPU 回归通过，其中包含 Cell 连接/范围、原有 refutation 组合。
- 关闭该开关时，3 个空表/零 atom 相关回归也通过。
- 新回归覆盖首次缺失、后续插入、删除、同批重插、union/rebuild、subsume、全批为空、零参数 relation，以及有限 Run 的冻结边界。
- 36 个 bench skill 测试通过。Cell/refutation/credit 对照现在要求两边通用 GPU 优化配置一致，不能把此收益记给 Cell。

## 使用

worker 协议为 layout 16，旧 worker 需要重建。默认开启；消融时设置：

```sh
EGGLOG_GPU_EMPTY_TABLE_PRUNING=0
```

用统一 skill 只读检查这次结果：

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py empty-tables \
  --before out/gpu-empty-depth-0 --after out/gpu-empty-depth-1 \
  --timing-before out/gpu-empty-time-0 --timing-after out/gpu-empty-time-1
```

[完整深度分布、逐规则剪枝和来源](results.json)。`gpu_optimizations.empty_table_pruning` 表示公共后端配置；`runtime.gpu_empty_table_prunes` 与每规则 `empty_table_prunes` 记录短路请求，是原 `dispatches` 的子集，不追加计算请求总数。

# eggcc If 超小子集：Cell 是否能利用结构稀疏性？

答案是：**当前版本没有加速；匹配的关键结构还没有被 Cell 接口简化。**
这里确实运行 native egglog，不是模拟 Cell，也没有退回 concrete-stage 缓存。

## 真实规则与输入

来源：`check_statewalk` 的 `ae80f82a403c1209fb084b115ee89470c7e66d0e`。
具体路径、文件哈希及改动范围见 `provenance.json`，MIT 许可见 `UPSTREAM-LICENSE`。

- `switch_rewrites.egg:5-17`：`if a<b then a else b → min(a,b)`。
- `switch_rewrites.egg:20-32`：`if a<b then b else a → max(a,b)`。
- `utility/util.egg:59`：`Get(Single(x),0) = x`，提供分支 tuple 的投影。

`rules.egg` 是可读的规则与所需 constructor 声明，`source.egg` 添加三个训练输入。
保留 Expr、Assumption、Type、TypeList、BaseType、BinaryOp 六个 equality sort，
保持原参数位置、分支上下文、索引一致性和类型连接。只裁去不用的声明，将三条
规则调度到单个 `if-tiny` ruleset。没有删 guard、改成单个 Math sort 或去掉 context。
这是一个单调正向 constructor/equality 子集，不含完整 eggcc 的类型分析、subst、
function/set/merge、循环和其他 If 优化。

训练程序分别包含 min、max 和不满足条件的 If。每个 If 的输入是二元整数 tuple，
两个分支各用 `Single(Get(Arg(type, InIf(...)), index))` 返回一个输出。
负例让两个分支都返回位置 0：它不匹配上述 min/max 规则，但在完整 eggcc 中
可能被其他规则优化，并不是全局无用节点。

测试使用全新的 `InFunc("heldout-if-i")`，不再 Bake。新图按 DAG let 节点生成：

- `all_match`：min/max 各半，所有 If 可被这两条规则之一优化。
- `sparse`：5% min、5% max、90% 近似但不匹配的结构（1000 If 时）。

“all_match”不表示图稠密：它的净新增节点/match 仍只有 1.334，Expr 节点/类为
1.25。`sparse` 对应两个数约 1.049 和 1.189。这里没有把 match 当作 probe。

## 实测：1000 个 If，单次运行

| 模式 | 全部可匹配：推理 ms | 10% 可匹配：推理 ms | 稀疏：总执行 ms | 稀疏：峰值 RSS MiB |
|---|---:|---:|---:|---:|
| pristine egglog | 44.302 | 41.983 | 895.058 | 253.02 |
| 当前裸内核 | 47.220 | 44.717 | 1105.797 | 288.00 |
| Cell 紧凑图 | 121.199 | 113.380 | 1251.553 | 296.06 |
| Cell 后完整导出 | 128.194 | 105.244 | 1277.926 | 297.28 |

每项独立进程，只跑一次。Cell 两行的推理时间是两次独立测量，不能相减当成
导出成本。显式导出单独计时：稀疏输入 55.516 ms。setup 是阶段边界，run 仍可能
包含延迟编译和 rebuild，不是纯 Join 时间。

输入阶段占约 1 秒：这个合成 IR 使用约 14,000 个命名 let，parser、全局绑定
和插入开销都在其中，RSS 也含这些元数据。因此别把 296 MiB 解释为仅 16K 个
e-node 的存储；原版和 Cell 使用同样的 DAG 命名结构。即使单看 run，Cell 仍慢
约 2.5 倍，不能把结果归因于输入格式后宣称获得推理收益。

Cell 前端从已有 AST 转换另需约 32 ms；离线采集/Bake/Gurobi/验证合计约
0.695 s。库文件 **117,812 B**，3 个 Cell 族、51 条消费者规则。库文本不等于
其解析后的内存，后者已包含在 RSS 中。

## 等效图规模：没有用模板乘法估算

以下数字在 pristine、当前裸内核、Cell 完整导出之间全部一致：

| 输入 | Expr e-node | Expr e-class | 全 sort e-node | 全 sort e-class |
|---|---:|---:|---:|---:|
| 1000 If，全部可匹配 | 15,000 | 12,000 | 18,008 | 15,008 |
| 1000 If，10% 可匹配 | 13,200 | 11,100 | 16,208 | 14,108 |

验证是逐行对应与 typed e-class 双射，包含 Assumption/Type 等全部六个 sort。
还独立检查每个 If：正例的 `Get(if,0)` 等价于对应 min/max；负例没有凭空生成
该输出。timing/RSS 进程不扫描全图，检查在其他进程完成；导出不重跑原规则。

稀疏输入实际保存 14,206 个 spine constructor 节点（其中 Expr 11,200），另有
2,100 条 Summary：1,900 活跃、200 subsumed。仅看物理行数量，节点省下的
部分已被 Summary 基本抵消，还没计 Summary 的列宽和索引。

## 这个例子还暴露并修复了什么

开始时生成的 Cell 导出只有 54 行，原始训练图应有 60 行，min/max 都没执行。
原因是父 `Single` Cell 已将内部 Get 隐藏，消费者却继续要求物理
`Get(Single(x),0)` 行存在。修复是从父闭包证书中找出结果等于可见 Summary 字段
的行，在 LHS 用字段投影代替该查询。处理是通用的，不硬编码 Single/Get/If。
RHS 仍保留所选 canonical spine 的物理物化契约。

修复后完整图相等，但核心 min/max 查询原来各有 **13 个不同 constructor
lookup 项**，生成的基础 Cell 消费者仍各有 **13 项**，并带额外的等价义务。
它主要封装了 Single 辅助闭包，If 的长条件仍需要重新匹配；继承又扩展成 51 条
消费者。因此“图更稀疏”本身没有解决当前 Cell 接口的粒度问题。

## 复现

```sh
out/cell-spine-venv/bin/python tools/bench/eggcc_if.py \
  --output out/eggcc-if-cell-next --ifs 10 1000
```

已有当前 native 工具时可加 `--skip-build`。默认会利用本地 clean baseline
worktree 和既有 Cargo 缓存重新构建 pristine runner，使用与当前 runner 完全
相同的 `examples/support/phased_bench.rs`。不修改 baseline 源码和旧已保存的
pristine 二进制。其他机器用 `--baseline PATH` 指定对应 clean checkout；没有
baseline 时可用 `--no-pristine`，保留当前内核的同二进制对照。

输出含 `index.html`、每阶段计时/RSS、完整图、每个 If 的真假例检查、库与生成
规则、源文件/二进制哈希。已保存 `results.json` 与 `measurements.md`。

```sh
out/cell-spine-venv/bin/python -m unittest tools.bench.test_eggcc_if -v
```

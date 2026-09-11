# math combine 历史接入 babble

真实调用固定版本的 upstream babble；不是原有手写 anti-unification 的改名。
详细配置、源码版本、兼容补丁和复现命令见
[`tools/babble_adapter/README.md`](../../tools/babble_adapter/README.md)。

输入来自已有 math microbenchmark 前 5 轮历史的 124 个组合类，携带 normalized
binding、producer/consumer 规则条件和动作（包含 union）、边界端口及源码
连接。每个类仅出现一次；原来的发生次数保留为元数据，不参与本轮加权。

每第五类留出：99 类学习、25 类测试。测试阶段只使用训练选择的库。
所有 124 个输入在编码后都能解码回原 JSON；训练和测试的库调用都经过
独立 lambda 展开器检查，与原输入 AST 完全相等。

## 本次结果

以下都是各自编码内的 AST 节点数，库定义及调用开销已计入：

| 编码 | 集合 | 原始 | babble 后 | 降幅 |
|---|---|---:|---:|---:|
| JSON 对照 | 学习 99 类 | 31695 | 26686 | 15.8% |
| JSON 对照 | 留出 25 类 | 7910 | 6763 | 14.5% |
| 紧凑 typed AST | 学习 99 类 | 8811 | 7237 | 17.9% |
| 紧凑 typed AST | 留出 25 类 | 2208 | 1847 | 16.3% |

JSON 对照学到的主要是类型标记和包装结构，所以不能只展示其压缩百分比。
紧凑编码将 `(sort,op,args)` 合为一个带 sort/op 标记的树节点，将 source
call/var/union 等编码为独立结构，仍然保留全部输入信息。

紧凑编码的两个库（当前运行 ID）：

| ID | 内容概述 | 学习集引用 | 留出集引用 |
|---|---|---:|---:|
| 583 | 带两个参数的规则外形：要求 root=L，动作 union(root,R) | 178 | 43 |
| 262 | producer p0 为固定 Mul 交换规则，producer p1 作为参数 | 10 | 3 |

引用数是在压缩后 corpus 中显式出现的库引用数，不是 runtime rule apply
次数，也不是覆盖的独立类数。精确定义见 `results.json` 的 `libraries`。
ID 583 还固定了局部端口标记和 naive=false；不要把上述概述解释为
无条件的新 rewrite。ID 262 表示历史中 producer 组合的语法片段。

这是独立的离线表示学习实验，不改变 egglog matcher、执行器或内核。
结果不是 e-graph 内存节省、优于完整 hashcons/DAG 存储的证明、跨运行泛化，
也不是 fractal 归纳证明。库参数是语法洞，尚未自动验证任意新实参的关系
类型或 trigger 条件。完整条件和动作保留为语法，不假定每个动作都曾提交。

## 产物

- `corpus.json`：输入和固定划分。
- `json-control.json`：普通 JSON 编码对照。
- `results.json`：紧凑编码指标、库定义和引用数。
- `*.programs.json`：完整库和调用程序，复现时生成，Git 不跟踪。

当前连接位置仍以历史 shape 字符串保留，尚未变成可单独学习的路径代数。
下一层输入改进应把这些连接结构化，并将学到的片段接回 BindingRelation
验证，而不是将这些有限语法抽象直接宣称为 fractal rule。

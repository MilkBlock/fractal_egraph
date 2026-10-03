# 用 cell 结构挑窄列，以及 cell 到底受什么限制

两个问题一起回答：**(1)** 怎么用我们已有的 RipSat cell / e-graph 去找"更窄的驱动列"；
**(2)** 现在的 cell 是不是因为深度限制而泛用性很弱。

数据：`/tmp/abl-e50`（events 发现 + `--rounds 3`，math-microbenchmark，92 个 cell）
和 `/tmp/eggcc-cell-reuse`（真实 eggcc 一次 pass，2,060 个 apply）。
脚本 `analyze_cells.py`（只读）。

## 一、cell 里现成就有的、挑列需要的东西

一个 cell 的 `origin` 里已经带着挑列所需的全部字段（实测字段名如下）：

| 字段 | 内容 | 挑列时的作用 |
|---|---|---|
| `reuse_interface_terms.lower/upper` | 边界端口，形如 `(RipenInput 0)` | **哪些位置是真正的跨 cell join key** |
| `comb_members[].binding` | `{"Input": port}` / `{"Parent": [member, out]}` | 区分 **boundary probe** 与 **cell 内部已推导**的位置 |
| `comb_members[].input_roles` | `var:a`、`read:body/0/expr/1`、`.../args/1` | **直接给出 relation + 列位置**（不用去 parse 规则文本） |
| `comb_members[].output_roles` | `column:head/0/expr/1:0`、`row:...` | 产出侧的列位置 |
| `comb_members[].aliases` | 端口 → token | 把 probe 值映射到符号值 |
| `symbolic_values` | token ↔ term ↔ **sort** | 判断该值是 bare `(RipenInput n)`（真 key）还是派生项/字面量 |
| `initial_tables` | cell 起始时每关系行数 | 局部规模 |
| `template` / `validation` | cell 闭包的符号行 | **每列 rows-per-key 的符号来源** |

关键点：**cell 区分 boundary 与 internal**。原始 trace（`observed-transitions.json`）
只给出"读了哪个 fact"，分不清这个读取是跨 cell 的 key 还是 cell 自己产出的东西；
cell 的 `binding` 直接标了。这是 cell 相对表统计的唯一不可替代的优势。

## 二、实测：boundary probe 排序（math，92 cell / 792 个 probe / 9 个 role）

| role（relation + 列路径） | occurrences | distinct keys | **hit multiplicity** | bare-boundary 占比 |
|---|---:|---:|---:|---:|
| `var:a` | 198 | 4 | **49.50** | 68% |
| `var:b` | 193 | 6 | **32.17** | 28% |
| `read:body/0/expr/1` | 221 | 7 | **31.57** | 23% |
| `var:x` | 54 | 5 | 10.80 | 56% |
| `read:body/0/expr/1/args/0` | 29 | 3 | 9.67 | 38% |
| `read:body/0/expr/1/args/1` | 51 | 7 | 7.29 | 20% |
| `var:c` | 32 | 5 | 6.40 | 6% |

读法：`var:a` 这个位置被 probe 198 次，只出现 **4 个不同值** → 每个 key 平均被用 **49.5 次**。
这就是"热 key"。`bare-boundary 占比` 说明该位置读到的值里有多少是纯 `(RipenInput n)`
（真正的跨 cell key），而不是 cell 自己算出来的派生项。

**这个 ranking 现在就能算**，不需要引擎统计。

## 三、评分公式（把"热"和"窄"分开）

派生一列窄 key 表值不值，取决于两个独立量：

```
worth(relation, column)
  ≈  hotness   (该列被 probe 的次数 / 不同 key 数)        ← cell 直接给（上表）
   × width     (rows(parent) / distinct(column))          ← 需要每列去重数
   − build     (rows(parent))                             ← 建一次 key 表
```

- **hotness** 高 = 同一个 key 被反复探 → 建一次 key 表能被摊薄很多次。
  上表 `var:a` = 49.5。
- **width** 高 = 每行 key 上挂着很多行 → 派生表比原表小得多（`|K| = distinct << rows`）。
- 两者都高才值得；只有 hotness 高而 width≈1（列近似唯一）时，派生表 ≈ 原表，
  只剩纯开销——这正是我之前担心的 `PotentialExtractedExpr expr` / `ContextOf e ctx` 那类列。

**缺口**：`width` 需要"每列 distinct 数 / 行数"，而 cell 的 `initial_tables` 只给 cell
**起始**行数（math 里除 `RipenInput: 7` 外全是 0），代表不了全局。
补法很便宜：给我已有的只读观察器再加一个"每表每列 distinct 计数"即可
（`materialized_indexes` 已经证明这个钩子能拿到 `TableInfo`）。

## 四、第二个问题：深度限制？——**不是深度，是闭包规模预算**

实测（math，92 cell）：

| state | n | cell 成员数 min/median/max | 闭包 facts |
|---|---:|---|---|
| Saturated | 70 | 1 / **3.0** / 10 | — |
| ObservedUnfinished | 22 | 1 / **4.0** / 8 | 14,151 / **18,457** / 32,097 |

**最深的 cell（10 个成员）全部 Saturated。** unfinished 的中位成员数只有 4，
却膨胀到 1.4 万–3.2 万 facts。所以：

- **结构深度不是限制**：最深 10 个成员照样闭合。
- **真正的限制是闭包规模预算**（`EGG_LAYOUT_RIPEN_OBSERVE_MS=50` /
  `_OBSERVE_FACTS=200000`）：22/92 撞的是 **time cap**（`reasons: ["time"]`），
  从一个 4 成员的 cell 膨胀到 3.2 万 facts。
- 换句话说：**cell 的"浅"是结构特征，"炸"是扇出特征**，两者解耦。

### 泛用性：分三种，别混在一起

1. **cell key 本身是程序无关的**：`parameter_marker` 的 `(RipenInput n)` 被
   alpha-renaming（`cell_binding_normalize::contract`），所以同一个 key 不依赖具体输入。
   这一点是好的。
2. **但 catalog 只覆盖观察到的 trace**：cell 是**观测**出来的，不是**推导**出来的。
   没被观察到的边界就没有 cell；README 也明确写了不会自动合成新 recipe。
   单程序单 schedule 的一次导出 ⇒ 覆盖率天然窄。
3. **在 eggcc 上现在是 0 个 cell。** `cell-egraph.json` 实测：

```json
{"registered_compositions":0,"contracts":0,"eclasses":0,"certified_unions":0,...}
```

2,060 个 apply 全被 `automatic merge update needs a pre-update table-state witness` 拒绝
（`native_ripen_entry.rs:636-644`：规则 head 里有 `set` 到带 `:merge` 的 function 就拒绝）。
eggcc 的规则恰恰到处是这种写法（`ExtractedExpr`、`Expr-size`、`tuple-length`、
`TypeList-length` …）。**所以在真实负载上不是"泛用性弱"，是"还没有 e-graph"。**

所以"深度限制导致泛用性弱"这个判断，我实测下来要改成：

- 深度不是瓶颈（深 cell 反而闭合）；
- 瓶颈一是 **闭包规模预算**（浅 cell 也能炸到 3.2 万 facts）；
- 瓶颈二是 **正确性门**（`:merge` 的 pre-update witness），它让 eggcc 上直接归零；
- 而 genericity 的真正受限处是**覆盖率**（只覆盖 trace），不是深度。

## 五、建议的下一步

1. **打通 eggcc 的 cell**（前置条件，否则第 2 步无从谈起）：给 `set` 到 merge function
   的动作记录"更新前的表状态"作为 witness。这是 codex 已经点名的那个独立限制。
2. **补 width 侧统计**：观察器加"每表每列 distinct 计数"，与本文的 hotness 相乘，
   得到 `worth(relation, column)` 排名。
3. **用 unfinished cell 当宽 key 的路标**：那 22 个撞 cap 的 cell 本身就是"某个 boundary
   port 扇出极大"的现成证据；把它们的 closure 按 boundary port 归因，
   就能直接列出最宽的列（最该避免 probe 的列），这比从表统计反推更早、更符号化。

产物：`analyze_cells.py`、`math3-events.json`（上述所有数字）。

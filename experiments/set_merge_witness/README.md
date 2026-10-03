# codex 的 `set` / merge-function 支持：机制、实测结果、现在的瓶颈

commit `6252dc0` 之后 codex 在**未提交的工作树**里做了这一轮
（`src/native_ripen_entry.rs`、`src/native_analyze.rs`、`src/native_history.rs`、
`egglog/core-relations/src/{trace.rs,table/mod.rs}` 等，最后修改时间 19:24–19:40）。
本文只读代码 + 实测，用 release 二进制重跑了真实 eggcc trace。

## 一、它做了什么：三值 witness + 带守卫的重放

### 1. 内核侧：merge 之前先把旧行抓下来

原来 `set` 写到带 `:merge` 的 function 上时，trace 只记「proposed / actual」，
没有更新前的值，所以重放无法复现这次写入。现在：

- `table/mod.rs:993-1026`：命中已存在的 key 时，先把当前行读出来
  `let before = cause.filter(|c| !c.effects_only()).map(|_| cur.to_vec());`，
  在执行 merge **之前**抓取，然后连同 merge 结果一起提交给 trace。
- `trace.rs:506-536` `record_write(..., before: Option<Option<&[Value]>>)`；
  `WriteEvent.before: Option<Option<Vec<Value>>>`，**三值语义**写得很清楚：

  | 值 | 含义 |
  |---|---|
  | `None` | 不支持/未知 —— 不允许猜 |
  | `Some(None)` | 更新前该行**不存在** |
  | `Some(Some(row))` | 更新前那一行的确切内容 |

- `trace.rs:714` 新增 `finish_with_before(...)`；旧的 `finish` 走 `before = None`，
  即**旧 history 不会被当成"有证据"**。

### 2. capture 侧：把 witness 落进 history

`native_analyze.rs:119-127`：

```rust
struct SetWitness { commit: u64, path: String, table: String,
                    before: PreviousRow, proposed: Vec<usize>, after: Vec<usize> }
enum PreviousRow { Unknown, Absent, Present(Vec<usize>) }
```

`native_analyze.rs:828-863`：只取 `source_span` 路径以 `/set/0` 结尾的写事件，
把 before/proposed/after 三行都 token 化，**按 commit 排序**存档。

### 3. replay 侧：每个 set 用 before/after 双守卫夹住

`native_ripen_entry.rs`：

- 按 `head/{action_index}/set/0` 找到该 action 的 witness，并且要求**恰好一次 commit**
  （`:404-413`）。
- `PreviousRow::Absent` → 用「双重 fail 即断言不存在」表示
  （`:441-443`，`immediate` 在 `:26-28`）。
- `PreviousRow::Present(old)` → 若这个 key 还没被写过，就**把它作为 seed 注入**
  （`:450-460`），并插入 `immediate(eq(target, old))` 作为 before 守卫。
- 动作序列变成 `before守卫 → Action → after守卫`（`set_guards`，`:466` / `:522-528`）。
- **`set` 产出的 output alias 用"提交时刻的 after 值"，不再去读那个可变 function**
  （`:539-542` 的注释写得很明确：a set result is a value at its commit）。
- 导出里新增 `set_update_witnesses`（`:771`）。
- **旧的 audit 门被删掉了**：`audit()`（`:777-800`）现在只检查
  source-rule coverage（bi-rewrite/include/push/pop/unconditional-rewrite），
  原来那条 `set` on merge function → `Err("automatic merge update needs a pre-update
  table-state witness")` 不见了。

### 4. 测试

```
two_sets_in_one_apply_keep_each_committed_scalar_snapshot   ok
set_replay_preserves_absence_old_values_and_stage_order      ok
```

第一个断言 `set_update_witnesses.len() == 2`，并且**故意**验证：
把 history 里的 `sets` 清空后 `prepare_members` 必须**报错**，即"没有 witness 不许猜"。

## 二、实测：门拆了，但 eggcc 上仍然是 0 个 cell

同一份真实 eggcc trace（`fact.bril --run-mode egglog --stop-after-n-passes 1`，
4,664 matches / 2,777 applications / 2,060 cells），release 二进制前后对比：

| | before（audit 门） | after（witness 机制） |
|---|---:|---:|
| live wall | **6.5 s** | **91.8 s** |
| replay wall | — | **108.9 s** |
| counts | `Rejected: 2060` | `Rejected: 2060` |
| certified cells | 0 | **0** |
| 拒绝原因种类 | 1 | 7 |

**慢 14 倍，产出仍然是零。** 原因分布变成：

| 原因 | 数量 | 占比 |
|---|---:|---:|
| `set commits interleave match order; stage replay cannot reorder them` | **1,194** | 58.0% |
| `automatic ripen cannot export its trigger state: unsupported container/table field sort` | **620** | 30.1% |
| `set witness needs a scalar literal or an existing symbolic binding` | 110 | 5.3% |
| `recorded output alias failed`（各 member） | ~85 | 4.1% |
| `external fact injection failed` | 40 | 1.9% |
| `unresolved binding inner` | 9 | 0.4% |

## 三、我对这三个主要瓶颈的读法

### (a) 58% 的 `set commits interleave match order` —— 很可能是**守卫范围过严**，不是本质限制

代码是（`native_ripen_entry.rs:216` + `:414-419`）：

```rust
let mut last_commit = 0;                    // ← 在遍历 members 之前初始化一次
for i in &members {                          // ← 每个 member 是一次独立的 apply
    ...
    for (action_index, action) in rule.rule.head.0.iter().enumerate() {
        ...
        if w.commit <= last_commit {
            return Err("set commits interleave match order; stage replay cannot reorder them");
        }
        last_commit = w.commit;
```

`last_commit` **跨 member 单调**，但它比较的 witness 来自**不同的 apply**（不同 record）。
不同 apply 的 commit 事件号当然不必按 discovery 顺序递增。实测支持这个判断：

| history 记录里 `sets` 的个数 | 记录数 |
|---|---:|
| 0 | 2,168 |
| 1 | **602** |
| 2 | **7** |
| 合计 | 2,777 |

**一个 record 里最多 2 个 set，而且只有 7 个 record 有 2 个。** 所以"同一次 match 内
action 交错"最多只能解释 7 个 record 的失败，**解释不了 1,194 个 cell 的失败** ——
绝大部分必然是跨 record 比较造成的。

可查的修法：把 `last_commit` **按 record 重置**（只在同一 apply 内要求 head 顺序），
或者先把 cell 的 members 按各自首个 commit 排序。这只在**同一 record 内**才需要
顺序保证，而那个场景只有 7 例。

### (b) 30% 的 `unsupported container/table field sort in state export`

ripen 的状态导出拒绝 container/presort 字段（multi-datatype README 里也写了
"exact ripen state export still rejects unsupported container fields rather than
mis-typing them"）。这是一个独立的导出能力缺口，和 witness 机制无关。

### (c) 5.3% 的 `set witness needs a scalar literal or an existing symbolic binding`

`literal()`（`:29-41`）只接受 pool 里的字面量或**已经存在的**符号绑定；
当旧值是尚未进入 pool 的 e-class 值时就无法在重放程序里命名它。
witness 抓到了，但**没法写进重放文本**。

## 四、这条链对整体进度意味着什么

- **好的部分**：`set`/merge 的语义缺口被真正补上了（三值 witness + 双守卫 + 提交时刻取值），
  而且"没有证据不许猜"这条纪律保住了（清空 sets 必须报错）。这是这一步最值钱的地方。
- **但 eggcc 上依旧 0 cell**，所以前面几轮需要 cell 才能做的测量
  （窄列挑选、转移复用、cell 复现率）在真实负载上**仍然无法做**。
  唯一不受影响的是我上一轮那个复用率测量，因为它只读
  `observed-transitions.json`，不需要 cell 通过认证。
- **下一步顺序建议**：先修 (a)（份额最大、证据最硬、改动最小），再看 (b) 的容器导出，
  最后 (c)。在那之前，任何"eggcc 上 cell 复用能省多少"的说法都没有数据支撑。

## 产物

| 文件 | 内容 |
|---|---|
| `summary.json` | 前后对比、拒绝原因直方图、history 的 set 分布 |
| `eggcc-after.json` | 这一轮的完整 report（live + replay） |

复现：

```sh
cargo build --offline --release --bin egg_layout
python3 experiments/multiple_datatypes/run.py \
  --eggcc /Users/mineralsteins/Repos/egg_related/check_statewalk \
  --analyzer target/release/egg_layout --output /tmp/eggcc-setmerge
```

注意：codex 这一轮改动**尚未提交**，仍在工作树里（且 `egglog/` 下混有更早的未提交改动）。

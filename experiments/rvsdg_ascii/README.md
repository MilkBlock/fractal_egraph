# eggcc RVSDG 真实 IR 的 ASCII 画法

对象：`check_statewalk`（eggcc，@ `ae80f82a`）里 Tiger 路径前端真正构造出来的 `RvsdgProgram`，
不是 tree/DAG encoding，也不是 `statewalkdp` 的 region。

## 1. 怎么拿到"真实 IR"

`--run-mode=rvsdg-conversion` 只把 `rvsdg.to_svg()` 打到 stdout；IR 本身没有任何文本 dump。
最小侵入的做法是加一个只读 helper（patch 见 `instrumented/rvsdg_debug_dump.patch`，
16 行，只加 `RvsdgProgram::debug_dump()` 并在 `RunMode::RvsdgConversion` 里 `eprint!`）：

```sh
cd /Users/mineralsteins/Repos/egg_related/check_statewalk
git apply /Users/mineralsteins/Repos/egg_related/egg_layout/experiments/rvsdg_ascii/instrumented/rvsdg_debug_dump.patch
LLVM_SYS_180_PREFIX=/opt/homebrew/opt/llvm@18 \
LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/cbc/lib:/opt/homebrew/opt/coinutils/lib \
  cargo build --release --offline
for f in add diamond fib_shape; do
  ./target/release/eggcc --run-mode=rvsdg-conversion tests/passing/small/$f.bril \
      >/tmp/$f.svg 2>experiments/rvsdg_ascii/ir/$f.ir.txt
done
```

stdout = SVG（可视化），stderr = 真实 IR（本文的原始数据）。
`ir/*.ir.txt` 是原样保存的 dump，本文所有图都由它推出，并且和 SVG 里的节点/区域一一对得上。

## 2. 画图前必须知道的 5 条规则（全部来自源码，不是猜的）

1. **节点表是平的**。`RvsdgFunction.nodes: Vec<RvsdgBody>`，节点只能引用下标更小的节点（`src/rvsdg/mod.rs:218`）。
   区域嵌套是隐式的，靠 `If`/`Gamma`/`Theta` 的 operand 列表表达。
2. **`Operand::Project(port, node_id)`** = 全局节点 `node_id` 的第 `port` 个输出口（`mod.rs:137-142`）。
3. **`Operand::Arg(i)` 是"最近的外层区域"的第 `i` 个入口**，不是函数的第 i 个参数。
   同一个 `Arg(1)` 在函数体里是 `PrintState`，在 loop 体里是 `i`。这是读 dump 最容易踩的坑。
4. **区域成员 = 从该区域 dst 出发沿"值边"可达**，并且**不穿过 branch body**
   （`rvsdg2svg.rs:687-710` 的 `reachable_nodes`；`If` 只走 `pred + inputs`，不走 `then/else_branch`）。
   所以 branch 里的节点只属于那个 branch 区域。
5. **端口编号**（`mk_node_and_input_edges`, `rvsdg2svg.rs:612-684`）：
   - `BasicOp(Op/Const/Effect)`：输入口 = operand 顺序；输出口 = `num_outputs()`（const/op 为 1，`Alloc`/`Load` 为 2）。
   - `Effect`（print 等）：**最后一个输入 = state 进，唯一输出 = state 出**。
   - `If{pred, inputs}`：节点输入口 = `[pred] ++ inputs`；两个 branch 区域的 `srcs` = `inputs`（**pred 不进 srcs**）；
     节点输出口数 = `then_branch.len()`，第 k 口 = branch 的 `dst k`。
   - `Theta{inputs}`：节点输入口 = `inputs`；loop body 区域 `srcs` = `inputs`，
     区域 `dsts` = `[pred] ++ outputs`，所以**节点输出口 p = 区域 dst p+1**。
     约定：loop 的 `in0` 是 state（SVG 把 loop 的第一入口画成黑点，`rvsdg2svg.rs:563`）。
6. 只有被 results 可达的节点才画出来。`fib_shape` 的 14 个节点里有 5 个（2,4,7,8,12）是死 const，
   所以 SVG 只出现 7 个 node 方块 —— dump 里有，图里没有，这不是丢失。

## 3. `add.bril`（最小例子：state 是 effect 的最后一个输入）

```bril
@main() { v0 = const 1; v1 = const 2; v2 = add v0 v1; print v2; }
```

```
main(s: PrintState) -> (s')

   (0) const 1        (1) const 2
       out0               out0
         |                  |
         +---------+--------+
                   v
              (2) add        in0 <- (0).out0
                |            in1 <- (1).out0
                |            out0 : Int
                v
              (3) print      in0 <- (2).out0        (被打印的值)
                |            in1 <- src0 = s       (STATE，永远是最后一个输入)
                |            out0 : PrintState     (STATE 作为唯一输出)
                v
        results[0] = (PrintState, (3).out0)
```

真实 dump（`ir/add.ir.txt`）：

```
function main args=[PrintState]
  [0] BasicOp(Const(Const, Int(1), Int))
  [1] BasicOp(Const(Const, Int(2), Int))
  [2] BasicOp(Op(Add, [Project(0, 0), Project(0, 1)], Int))
  [3] BasicOp(Effect(Print, [Project(0, 2), Arg(0)]))
  results=[(PrintState, Project(0, 3))]
```

## 4. `diamond.bril`（Gamma：`if` 区域 + 两条 branch，分支返回 state）

```bril
@main(x: int) { cond = lt x x; br cond .B .C;
  .B: a = const 1; print a; jmp .D;
  .C: a = const 2; print a; jmp .D; .D: ret; }
```

```
                         main(x: int, s: PrintState) -> (s')

  src0 (x) ---+-------------------------------+
              |                               |
              v                               |
        +-----------+                         |
        |  (0) lt   |   in0 <- src0 (x)       |
        +-----+-----+   in1 <- src0 (x)       |
              | out0 : Bool                   |
              | pred                          |
              v                               |
  +-----------+-------------------------------------------+----------+
  |  (5) if   <Gamma>                                                |
  |     in0 <- (0).out0        (pred)                                |
  |     in1 <- src1 = s        (Gamma 的 input, 即被 merge 的值)      |
  |     out0 = (5).out0 : PrintState  -> results[0]                  |
  |                                                                  |
  |  +-- branch TRUE : srcs = (5).inputs = [s] ----------------+      |
  |  |    +-----------+                                        |      |
  |  |    | (3) const |   out0 : Int                           |      |
  |  |    |     1     |                                        |      |
  |  |    +-----+-----+                                        |      |
  |  |          | out0                                         |      |
  |  |          v                                              |      |
  |  |    +-----------+   in0 <- (3).out0                      |      |
  |  |    | (4) print |   in1 <- src0  ( = s , STATE)          |      |
  |  |    +-----------+   out0 = dst0  -> (5).out0             |      |
  |  +---------------------------------------------------------+      |
  |                                                                  |
  |  +-- branch FALSE : srcs = (5).inputs = [s] ---------------+      |
  |  |    +-----------+                                        |      |
  |  |    | (1) const |   out0 : Int                           |      |
  |  |    |     2     |                                        |      |
  |  |    +-----+-----+                                        |      |
  |  |          | out0                                         |      |
  |  |          v                                              |      |
  |  |    +-----------+   in0 <- (1).out0                      |      |
  |  |    | (2) print |   in1 <- src0  ( = s , STATE)          |      |
  |  |    +-----------+   out0 = dst0  -> (5).out0             |      |
  |  +---------------------------------------------------------+      |
  +------------------------------------------------------------------+
```

注意两点：

* `if` 节点自己的输入口是 `[pred, src1]`，但两个 branch 区域的 `srcs` **只有 `[src1]`**，
  所以 branch 里的 `Arg(0)`（图上写作 `src0`）指的是 `s`，不是 `x`。这就是 `(2) print in1 <- Arg(0)`
  在 dump 里看起来"类型不对"的原因。
* 两个分支都返回一路 state，Gamma 把它 merge 成 `(5).out0`，函数结果就是它。

## 5. `fib_shape.bril`（Theta + 内嵌 Gamma：循环谓词由分支输出决定）

```bril
@main(input: int) { one = const 1; i = const 0;
  .loop: cond = lt i input; br cond .body .done;
  .body: i = add i one; bodyvar = const 1; jmp .loop;
  .done: donebody = const 1; print i; }
```

```
                       main(input: int, s: PrintState) -> (s')

  (0) const 1 --out0--> (11).in2      (字面量 one)
  (1) const 0 --out0--> (11).in1      (i 的初值)

  +======================================================================+
  |  (11) theta   <loop>                                                 |
  |    loop-carried（in/out 同序，端口 0 是 STATE）                        |
  |      in0 = src1 (s)        ->  out0  = (10).out0   (s')              |
  |      in1 = (1).out0 (0)    ->  out1  = (10).out1   (i')              |
  |      in2 = (0).out0 (1)    ->  out2  = (10).out3   (one)             |
  |      in3 = src0 (input)    ->  out3  = (10).out4   (input)           |
  |    pred = body dst0 = (10).out2     <-- 循环继续与否由 if 的 out2 给    |
  |                                                                      |
  |    +--------------------------------------------------------------+  |
  |    | loop body region    srcs = (11).inputs = [s, i, one, input]   |  |
  |    |   src0 = s    src1 = i    src2 = one    src3 = input          |  |
  |    |   dst0 -> (11).pred     dst1..4 -> (11).out0..3               |  |
  |    |                                                              |  |
  |    |      src1 (i)     src3 (input)                               |  |
  |    |          |            |                                      |  |
  |    |          v            v                                      |  |
  |    |      +-----------------+                                     |  |
  |    |      |     (3) lt      |   in0 <- src1 (i)                   |  |
  |    |      +--------+--------+   in1 <- src3 (input)               |  |
  |    |               | out0 : Bool                                 |  |
  |    |               | pred                                        |  |
  |    |               v                                             |  |
  |    |   +---------------------------------------------+            |  |
  |    |   |  (10) if   <Gamma>                          |            |  |
  |    |   |    in0 <- (3).out0        (pred)            |            |  |
  |    |   |    in1 <- src0 (s)   in2 <- src1 (i)        |            |  |
  |    |   |    in3 <- src2 (one) in4 <- src3 (input)    |            |  |
  |    |   |                                             |            |  |
  |    |   |  +-- TRUE（继续循环）srcs = (10).inputs --+  |            |  |
  |    |   |  | (6) add   in0 <- src1 (i)             |  |            |  |
  |    |   |  |           in1 <- src2 (one)           |  |            |  |
  |    |   |  |           out0 = i+1                 |  |            |  |
  |    |   |  | (9) const true    out0               |  |            |  |
  |    |   |  | dst = [ src0(s), (6).out0,           |  |            |  |
  |    |   |  |         (9).out0, src2(one),         |  |            |  |
  |    |   |  |         src3(input) ]                |  |            |  |
  |    |   |  +--------------------------------------+  |            |  |
  |    |   |  +-- FALSE（退出循环）srcs = (10).inputs+  |            |  |
  |    |   |  | (5) const false   out0               |  |            |  |
  |    |   |  | dst = [ src0(s), src1(i),            |  |            |  |
  |    |   |  |         (5).out0, src2(one),         |  |            |  |
  |    |   |  |         src3(input) ]                |  |            |  |
  |    |   |  +--------------------------------------+  |            |  |
  |    |   +---------------------------------------------+            |  |
  |    +--------------------------------------------------------------+  |
  +======================================================================+
                   |                              |
                   v                              v
        (13) print   in0 <- (11).out1 (i')      in1 <- (11).out0 (s')  [STATE]
                     out0 -> results[0] : PrintState
```

`If` 的 5 个输出口（`If.out k = branch dst k`），以及它们怎么接回 Theta：

```
out0 = state   : TRUE 分支返回 src0 = s            FALSE 分支返回 src0 = s
out1 = i'      : TRUE 分支返回 (6).out0 = i+one     FALSE 分支返回 src1 = i
out2 = 继续标记 : TRUE 分支返回 (9).out0 = true      FALSE 分支返回 (5).out0 = false
out3 = one     : 两边都原样返回 src2
out4 = input   : 两边都原样返回 src3
```

所以 `br cond .body .done` 在这个 IR 里不是"两个 block 二选一"，而是：

1. `lt i input`（node 3）只当 `if` 的 pred，用完就没了；
2. `if` **额外多出第 2 号输出**（`true`/`false`），它才是 theta 的 `pred`；
3. theta 把 `out0,out1,out3,out4`（**跳过 out2**）当作下一轮/最终的 loop-carried 值，
   这也正是 `Theta.outputs = [Project(0,10), Project(1,10), Project(3,10), Project(4,10)]` 里
   端口号从 0,1 直接跳到 3,4 的原因。

## 6. 和 `statewalkdp` 的 region 是两回事

* 上面这些 `Gamma`/`Theta` 是**语法结构化**的结果：CFG restructure 之后按支配关系切的。
* `statewalkdp.cpp` 的 region 是**语义/代价驱动**的：从 `isEffectful` 的 eclass 沿"第一个 effectful 子节点"
  的脊椎切，纯节点作为 fringe，第二个 effectful 子节点再切成子 region。
* 一个 ripsat cell 通常跨多个上面的 Gamma/Theta region；两者节点数、边界都不对应，不能互相套用。
* 这里画的 state 边（`PrintState`）就是 Tiger 里 `StateT` 在 RVSDG 前端的样子：
  effect 的最后一个输入/唯一输出，loop 的第 0 个进/出口。

## 7. 复现清单

| 文件 | 内容 |
| --- | --- |
| `instrumented/rvsdg_debug_dump.patch` | 给 check_statewalk 加 `RvsdgProgram::debug_dump()`（16 行） |
| `ir/add.ir.txt` | `add.bril` 的真实 IR dump |
| `ir/diamond.ir.txt` | `diamond.bril` 的真实 IR dump |
| `ir/fib_shape.ir.txt` | `fib_shape.bril` 的真实 IR dump |

`check_statewalk` 的工作树在生成这些文件后已 `git checkout` 回 `ae80f82a` 的干净状态；
所有改动只以 patch 形式保存在本目录。

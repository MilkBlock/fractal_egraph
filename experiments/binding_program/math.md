# Math BindingMap 构件与关系

124 个源位置组合类 → 100 个归一化端口关系。582 次历史实例通过连接验证。

这些关系保留类型、未赋值的 consumer 端口和嵌套等价约束。物化节点的值来自原生见证；不是从原输入独立重算出的结果。共享 wiring 不代表原始 rule 的 guard 或图更新效果等价。

## 发现的纯路由关系

| 第一映射 | 第二映射 | 合成结果 |
|---|---|---|
| B0 | B0 | `Identity` |
| B2 | B0 | `o0 := i1; o1 := i0` |
| B36 | B0 | `o0 := i2; o1 := i0` |
| B60 | B0 | `o0 := i2; o1 := i1` |
| B67 | B0 | `B67` |

B0 是 `(i0,i1) → (i1,i0)`；B67 是 `i0 → (i0,i0)`。所以 B0;B0 为恒等路由，B67;B0 仍为 B67。证明来自端口代入，不是运行时值偶然相等；但没有将此等式提升为完整 e-graph 更新过程的等价。

## 构件选择

| 构件 | 引入时替换次数 | 扣除定义成本后减少的 AST 节点 |
|---|---:|---:|
| F0 | 28 | 261 |
| F1 | 18 | 197 |
| F2 | 3 | 58 |
| F3 | 8 | 57 |
| F4 | 8 | 50 |
| F5 | 5 | 46 |
| F6 | 5 | 38 |
| F7 | 4 | 32 |

### F0

```text
binding-map($0:Signature, outputs(Math, Math, Math), ops(o0 := $1:Math, o1 := $2:Math, o2 := $3:Math, require $4:Math ≡ Mul@witness($5:Math, $6:Math)))
```

### F1

```text
binding-map($0:Signature, outputs(Math, Math, Math), ops(o0 := $1:Math, o1 := $2:Math, o2 := $3:Math, require $4:Math ≡ Add@witness(o1, o2)))
```

### F2

```text
binding-map(inputs(Math, Math, Math, Math, Math, Math), outputs(Math, Math, Math), ops(o0 := $0:Math, o1 := Mul@witness(i1, Diff@witness(i0, i2)), o2 := Mul@witness(i2, Diff@witness(i0, i1)), require Add@witness(o1, o2) ≡ Diff@witness(i3, $1:Math)))
```

### F3

```text
call:F0(inputs(Math, Math, Math, Math, Math, Math), $0:Math, $1:Math, $2:Math, $3:Math, o1, o2)
```

### F4

```text
call:F0(inputs(Math, Math, Math, Math, Math), $0:Math, $1:Math, $2:Math, $3:Math, o1, o2)
```

### F5

```text
binding-map($0:Signature, outputs(Math, Math, Math), ops(o0 := $1:Math, o1 := $2:Math, o2 := $3:Math, require $4:Math ≡ Mul@witness(o0, $5:Math), require $6:Math ≡ Mul@witness(o0, $7:Math)))
```

### F6

```text
binding-map(inputs(Math, Math, Math), outputs(Math, Math, Math), ops($0:Assignment, require $1:Math ≡ Mul@witness($2:Math, $3:Math)))
```

### F7

```text
binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := $0:Math, o1 := Integral@witness($1:Math, i2)))
```

这些是有类型的语法展开宏，尚未证明为递归/fractal rule。F 引用其他 F 时构成无环定义图；每一轮选择后都展开回原始 BindingMap 并比较完全相等。新调用仍需检查作用域和接口约束。

## 描述长度

| 表示 | AST 节点单位 |
|---|---:|
| 124 个原始映射 | 2683 |
| 归一化去重 + 类别引用 | 2355 |
| 构件库 + 分解 + 类别引用 | 1616 |

这是映射表示的 AST 节点计数，不是实际字节、egraph 内存或完整证明存储。类型签名-only 的压缩不计入主实验；无过滤语法压缩对照的独立映射库成本为 1167 节点，说明更高的纯语法压缩率不自动意味着更有意义的规律。

## 全部组合的分解

| 原排名 | BindingMap | 归一化映射 | 未赋值 consumer 端口 | 构件表达式 |
|---:|---|---|---|---|
| 1 | B0 | `o0 := i1; o1 := i0` | [] | `binding-map(inputs(Math, Math), outputs(Math, Math), ops(o0 := i1, o1 := i0))` |
| 2 | B0 | `o0 := i1; o1 := i0` | [] | `binding-map(inputs(Math, Math), outputs(Math, Math), ops(o0 := i1, o1 := i0))` |
| 3 | B1 | `o0 := i3; o1 := i1; o2 := i0; require i2 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math), i3, i1, i0, i2)` |
| 4 | B2 | `o0 := i0; o1 := i1` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := i0, o1 := i1))` |
| 5 | B3 | `o0 := Add@witness(i0, i1); o1 := i2` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := Add@witness(i0, i1), o1 := i2))` |
| 6 | B4 | `o0 := i4; o1 := Add@witness(i0, i1); o2 := i2; require i3 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math), i4, Add@witness(i0, i1), i2, i3)` |
| 7 | B5 | `o0 := Mul@witness(i1, Diff@witness(i0, i2)); o1 := Mul@witness(i2, Diff@witness(i0, i1))` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := Mul@witness(i1, Diff@witness(i0, i2)), o1 := Mul@witness(i2, Diff@witness(i0, i1))))` |
| 8 | B6 | `o0 := i3; o1 := i1; o2 := i0; require i2 ≡ Mul@witness(o1, o2)` | [] | `call:F0(inputs(Math, Math, Math, Math), i3, i1, i0, i2, o1, o2)` |
| 9 | B7 | `o0 := Mul@witness(i0, Integral@witness(i1, i2)); o1 := Integral@witness(Mul@witness(Diff@witness(i2, i0), Integral@witness(i1, i2)), i2)` | [] | `call:F7(Mul@witness(i0, Integral@witness(i1, i2)), Mul@witness(Diff@witness(i2, i0), Integral@witness(i1, i2)))` |
| 10 | B8 | `o0 := i4; o1 := i0; o2 := i1; require i3 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math), i4, i0, i1, i3)` |
| 11 | B9 | `o0 := Mul@witness(i0, i1); o1 := i2` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := Mul@witness(i0, i1), o1 := i2))` |
| 12 | B10 | `o0 := i0; o1 := Integral@witness(i1, i2)` | [] | `call:F7(i0, i1)` |
| 13 | B11 | `o0 := i1; o1 := Diff@witness(i0, i2)` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := i1, o1 := Diff@witness(i0, i2)))` |
| 14 | B12 | `o0 := i2; o1 := Diff@witness(i0, i1)` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := i2, o1 := Diff@witness(i0, i1)))` |
| 15 | B13 | `o0 := i2; o1 := i1; o2 := i0; require i3 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math), i2, i1, i0, i3)` |
| 16 | B14 | `o0 := i5; o1 := Mul@witness(i1, Diff@witness(i0, i2)); o2 := Mul@witness(i2, Diff@witness(i0, i1)); require Add@witness(o1, o2) ≡ Diff@witness(i3, i4)` | [] | `call:F2(i5, i4)` |
| 17 | B15 | `o0 := Diff@witness(i2, i0); o1 := Integral@witness(i1, i2); o2 := i2; require Mul@witness(Diff@witness(i2, i0), Integral@witness(i1, i2)) ≡ Mul@witness(o0, o1)` | [] | `call:F0(inputs(Math, Math, Math), Diff@witness(i2, i0), Integral@witness(i1, i2), i2, Mul@witness(Diff@witness(i2, i0), Integral@witness(i1, i2)), o0, o1)` |
| 18 | B16 | `o0 := Diff@witness(i2, i0); o1 := Integral@witness(i1, i2)` | [] | `call:F7(Diff@witness(i2, i0), i1)` |
| 19 | B2 | `o0 := i0; o1 := i1` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := i0, o1 := i1))` |
| 20 | B17 | `o0 := i0; o1 := Mul@witness(Const@witness(literal:Int(-1)), i1)` | [] | `binding-map(inputs(Math, Math), outputs(Math, Math), ops(o0 := i0, o1 := Mul@witness(Const@witness(literal:Int(-1)), i1)))` |
| 21 | B18 | `o0 := Const@witness(literal:Int(-1)); o1 := i1` | [] | `binding-map(inputs(Math, Math), outputs(Math, Math), ops(o0 := Const@witness(literal:Int(-1)), o1 := i1))` |
| 22 | B19 | `o0 := i3; o1 := i0; o2 := i1; require i4 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math, Math), i3, i0, i1, i4)` |
| 23 | B20 | `o0 := Diff@witness(i3, i4); o1 := Mul@witness(i1, Diff@witness(i0, i2)); o2 := Mul@witness(i2, Diff@witness(i0, i1)); require Add@witness(o1, o2) ≡ Diff@witness(i3, i5)` | [] | `call:F2(Diff@witness(i3, i4), i5)` |
| 24 | B21 | `o0 := i4; o1 := i0; o2 := i1; require i3 ≡ Mul@witness(o1, o2)` | [] | `call:F4(i4, i0, i1, i3)` |
| 25 | B22 | `o0 := i4; o1 := Mul@witness(i0, i1); o2 := Mul@witness(i0, i2); require i3 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math), i4, Mul@witness(i0, i1), Mul@witness(i0, i2), i3)` |
| 26 | B23 | `o0 := i1; o1 := i0; o2 := i4; require Mul@witness(Diff@witness(i4, i2), Integral@witness(i3, i4)) ≡ Mul@witness(o0, o1)` | [] | `call:F0(inputs(Math, Math, Math, Math, Math), i1, i0, i4, Mul@witness(Diff@witness(i4, i2), Integral@witness(i3, i4)), o0, o1)` |
| 27 | B24 | `o0 := i2; o1 := i1; o2 := i0; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F4(i2, i1, i0, i4)` |
| 28 | B25 | `o0 := i2; o1 := i1; o2 := i0; require i3 ≡ Mul@witness(o1, o2)` | [] | `call:F4(i2, i1, i0, i3)` |
| 29 | B26 | `o2 := i2; require Mul@witness(Diff@witness(i2, i0), Integral@witness(i1, i2)) ≡ Mul@witness(o0, o1)` | [0, 1] | `call:F6(o2 := i2, Mul@witness(Diff@witness(i2, i0), Integral@witness(i1, i2)), o0, o1)` |
| 30 | B27 | `o0 := i3; o1 := Mul@witness(i0, i1); o2 := i2; require i5 ≡ Mul@witness(o1, o2)` | [] | `call:F3(i3, Mul@witness(i0, i1), i2, i5)` |
| 31 | B28 | `o0 := i1; require i0 ≡ Mul@witness(o1, o2)` | [1, 2] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o0 := i1, require i0 ≡ Mul@witness(o1, o2)))` |
| 32 | B29 | `o1 := i1; o2 := i0` | [0] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o1 := i1, o2 := i0))` |
| 33 | B30 | `o0 := i4; o1 := Mul@witness(i0, i1); o2 := i2; require i3 ≡ Mul@witness(o1, o2)` | [] | `call:F4(i4, Mul@witness(i0, i1), i2, i3)` |
| 34 | B31 | `o0 := Const@witness(literal:Int(-1)); o1 := i0; o2 := Mul@witness(Const@witness(literal:Int(-1)), i1); require i3 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math), Const@witness(literal:Int(-1)), i0, Mul@witness(Const@witness(literal:Int(-1)), i1), i3)` |
| 35 | B32 | `o0 := Mul@witness(i0, i1); o1 := Mul@witness(i0, i2)` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := Mul@witness(i0, i1), o1 := Mul@witness(i0, i2)))` |
| 36 | B2 | `o0 := i0; o1 := i1` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := i0, o1 := i1))` |
| 37 | B33 | `o0 := Diff@witness(i0, i1); o1 := Diff@witness(i0, i2)` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := Diff@witness(i0, i1), o1 := Diff@witness(i0, i2)))` |
| 38 | B34 | `o1 := Add@witness(i0, i1); o2 := i2` | [0] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math, Math), ops(o1 := Add@witness(i0, i1), o2 := i2))` |
| 39 | B35 | `o0 := i0; o1 := i1; o2 := i2; require Mul@witness(i0, i1) ≡ Mul@witness(o0, o1); require Mul@witness(i0, i2) ≡ Mul@witness(o0, o2)` | [] | `call:F5(inputs(Math, Math, Math), i0, i1, i2, Mul@witness(i0, i1), o1, Mul@witness(i0, i2), o2)` |
| 40 | B36 | `o0 := i0; o1 := i2` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := i0, o1 := i2))` |
| 41 | B37 | `o0 := Diff@witness(i2, i3); o1 := i1; o2 := i0; require Add@witness(o1, o2) ≡ Diff@witness(i2, i4)` | [] | `binding-map(inputs(Math, Math, Math, Math, Math), outputs(Math, Math, Math), ops(o0 := Diff@witness(i2, i3), o1 := i1, o2 := i0, require Add@witness(o1, o2) ≡ Diff@witness(i2, i4)))` |
| 42 | B1 | `o0 := i3; o1 := i1; o2 := i0; require i2 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math), i3, i1, i0, i2)` |
| 43 | B38 | `o0 := Add@witness(i2, i3); o1 := i1; o2 := i0; require i4 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math), Add@witness(i2, i3), i1, i0, i4)` |
| 44 | B29 | `o1 := i1; o2 := i0` | [0] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o1 := i1, o2 := i0))` |
| 45 | B39 | `o0 := i1; require i0 ≡ Add@witness(o1, o2)` | [1, 2] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o0 := i1, require i0 ≡ Add@witness(o1, o2)))` |
| 46 | B40 | `o0 := i0; require i1 ≡ Mul@witness(o1, o2)` | [1, 2] | `call:F6(o0 := i0, i1, o1, o2)` |
| 47 | B41 | `o0 := i4; o1 := Mul@witness(i1, Diff@witness(i0, i2)); o2 := Mul@witness(i2, Diff@witness(i0, i1)); require Add@witness(o1, o2) ≡ Diff@witness(i3, i5)` | [] | `call:F2(i4, i5)` |
| 48 | B42 | `o0 := i4; o1 := Mul@witness(i1, Diff@witness(i0, i2)); o2 := Mul@witness(i2, Diff@witness(i0, i1)); require i3 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math), i4, Mul@witness(i1, Diff@witness(i0, i2)), Mul@witness(i2, Diff@witness(i0, i1)), i3)` |
| 49 | B43 | `o0 := i4; o1 := i1; o2 := i0; require i2 ≡ Mul@witness(o1, o2)` | [] | `call:F4(i4, i1, i0, i2)` |
| 50 | B25 | `o0 := i2; o1 := i1; o2 := i0; require i3 ≡ Mul@witness(o1, o2)` | [] | `call:F4(i2, i1, i0, i3)` |
| 51 | B44 | `o0 := Const@witness(literal:Int(-1)); o1 := i1; o2 := i0; require i3 ≡ Mul@witness(o1, o2)` | [] | `call:F0(inputs(Math, Math, Math, Math), Const@witness(literal:Int(-1)), i1, i0, i3, o1, o2)` |
| 52 | B24 | `o0 := i2; o1 := i1; o2 := i0; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F4(i2, i1, i0, i4)` |
| 53 | B45 | `o0 := Mul@witness(i0, Integral@witness(i1, i2)); o1 := Integral@witness(Mul@witness(Diff@witness(i2, i0), Integral@witness(i1, i2)), i2); o2 := i5; require i4 ≡ Sub@witness(o0, o1)` | [] | `binding-map(inputs(Math, Math, Math, Math, Math, Math), outputs(Math, Math, Math), ops(o0 := Mul@witness(i0, Integral@witness(i1, i2)), o1 := Integral@witness(Mul@witness(Diff@witness(i2, i0), Integral@witness(i1, i2)), i2), o2 := i5, require i4 ≡ Sub@witness(o0, o1)))` |
| 54 | B46 | `o0 := i0; require i1 ≡ Add@witness(o1, o2)` | [1, 2] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math, Math), ops(o0 := i0, require i1 ≡ Add@witness(o1, o2)))` |
| 55 | B47 | `o0 := Add@witness(i0, i1); require i2 ≡ Add@witness(o1, o2)` | [1, 2] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math, Math), ops(o0 := Add@witness(i0, i1), require i2 ≡ Add@witness(o1, o2)))` |
| 56 | B48 | `o0 := i3; o1 := i0; o2 := i1; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F3(i3, i0, i1, i4)` |
| 57 | B49 | `o0 := Mul@witness(i0, i1); o1 := i2; o2 := i5; require Mul@witness(Diff@witness(i5, i3), Integral@witness(i4, i5)) ≡ Mul@witness(o0, o1)` | [] | `call:F0(inputs(Math, Math, Math, Math, Math, Math), Mul@witness(i0, i1), i2, i5, Mul@witness(Diff@witness(i5, i3), Integral@witness(i4, i5)), o0, o1)` |
| 58 | B50 | `o0 := i3; o1 := Mul@witness(i0, i1); o2 := i2; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F3(i3, Mul@witness(i0, i1), i2, i4)` |
| 59 | B50 | `o0 := i3; o1 := Mul@witness(i0, i1); o2 := i2; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F3(i3, Mul@witness(i0, i1), i2, i4)` |
| 60 | B51 | `o0 := i3; o1 := i0; o2 := Mul@witness(Const@witness(literal:Int(-1)), i1); require i2 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math), i3, i0, Mul@witness(Const@witness(literal:Int(-1)), i1), i2)` |
| 61 | B52 | `o0 := i3; o1 := Mul@witness(i0, i1); o2 := Mul@witness(i0, i2); require i4 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math, Math), i3, Mul@witness(i0, i1), Mul@witness(i0, i2), i4)` |
| 62 | B53 | `o0 := Add@witness(i3, i4); o1 := Mul@witness(i0, i1); o2 := Mul@witness(i0, i2); require i5 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math, Math), Add@witness(i3, i4), Mul@witness(i0, i1), Mul@witness(i0, i2), i5)` |
| 63 | B54 | `o0 := i3; o1 := Mul@witness(i0, i1); o2 := Mul@witness(i0, i2); require Add@witness(o1, o2) ≡ Mul@witness(Const@witness(literal:Int(-1)), i4)` | [] | `call:F0(inputs(Math, Math, Math, Math, Math), i3, Mul@witness(i0, i1), Mul@witness(i0, i2), Add@witness(o1, o2), Const@witness(literal:Int(-1)), i4)` |
| 64 | B40 | `o0 := i0; require i1 ≡ Mul@witness(o1, o2)` | [1, 2] | `call:F6(o0 := i0, i1, o1, o2)` |
| 65 | B55 | `o0 := i1; o1 := i0; o2 := i4; require i3 ≡ Add@witness(o0, o1)` | [] | `binding-map(inputs(Math, Math, Math, Math, Math), outputs(Math, Math, Math), ops(o0 := i1, o1 := i0, o2 := i4, require i3 ≡ Add@witness(o0, o1)))` |
| 66 | B56 | `o0 := i1; o1 := i0` | [2] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o0 := i1, o1 := i0))` |
| 67 | B57 | `o0 := i4; o1 := i1; o2 := i0; require i2 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math), i4, i1, i0, i2)` |
| 68 | B58 | `o0 := Const@witness(literal:Int(-1)); o1 := i1; o2 := i0; require i3 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math), Const@witness(literal:Int(-1)), i1, i0, i3)` |
| 69 | B29 | `o1 := i1; o2 := i0` | [0] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o1 := i1, o2 := i0))` |
| 70 | B59 | `o0 := i0; o1 := i1; o2 := i2; require Add@witness(i1, i2) ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math), i0, i1, i2, Add@witness(i1, i2))` |
| 71 | B60 | `o0 := i1; o1 := i2` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := i1, o1 := i2))` |
| 72 | B61 | `o0 := i0; o1 := Add@witness(i1, i2)` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math), ops(o0 := i0, o1 := Add@witness(i1, i2)))` |
| 73 | B62 | `o0 := i1; o1 := i0; o2 := i0; require i3 ≡ Mul@witness(o1, o2)` | [] | `call:F0(inputs(Math, Math, Math, Math), i1, i0, i0, i3, o1, o2)` |
| 74 | B63 | `o0 := i2; o1 := i0; o2 := i0; require i1 ≡ Mul@witness(o1, o2)` | [] | `call:F0(inputs(Math, Math, Math), i2, i0, i0, i1, o1, o2)` |
| 75 | B64 | `o0 := i1; o1 := i0; o2 := i0; require i2 ≡ Mul@witness(o1, o2)` | [] | `call:F0(inputs(Math, Math, Math, Math), i1, i0, i0, i2, o1, o2)` |
| 76 | B65 | `o0 := Mul@witness(i1, i2); o1 := i0; o2 := i0; require i3 ≡ Mul@witness(o1, o2)` | [] | `call:F0(inputs(Math, Math, Math, Math), Mul@witness(i1, i2), i0, i0, i3, o1, o2)` |
| 77 | B66 | `o1 := i0; o2 := i0` | [0] | `binding-map(inputs(Math), outputs(Math, Math, Math), ops(o1 := i0, o2 := i0))` |
| 78 | B67 | `o0 := i0; o1 := i0` | [] | `binding-map(inputs(Math), outputs(Math, Math), ops(o0 := i0, o1 := i0))` |
| 79 | B68 | `o0 := i4; o1 := Diff@witness(i0, i1); o2 := Diff@witness(i0, i2); require i3 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math), i4, Diff@witness(i0, i1), Diff@witness(i0, i2), i3)` |
| 80 | B69 | `o0 := i5; o1 := Diff@witness(i0, i1); o2 := Diff@witness(i0, i2); require i3 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math, Math), i5, Diff@witness(i0, i1), Diff@witness(i0, i2), i3)` |
| 81 | B70 | `o0 := i0; require i2 ≡ Mul@witness(o1, o2)` | [1, 2] | `call:F6(o0 := i0, i2, o1, o2)` |
| 82 | B71 | `o0 := Mul@witness(i1, Diff@witness(i0, i2)); o1 := Mul@witness(i2, Diff@witness(i0, i1)); o2 := i5; require i4 ≡ Add@witness(o0, o1)` | [] | `binding-map(inputs(Math, Math, Math, Math, Math, Math), outputs(Math, Math, Math), ops(o0 := Mul@witness(i1, Diff@witness(i0, i2)), o1 := Mul@witness(i2, Diff@witness(i0, i1)), o2 := i5, require i4 ≡ Add@witness(o0, o1)))` |
| 83 | B72 | `o0 := i5; o1 := Mul@witness(i1, Diff@witness(i0, i2)); o2 := Mul@witness(i2, Diff@witness(i0, i1)); require i3 ≡ Add@witness(o1, o2)` | [] | `call:F1(inputs(Math, Math, Math, Math, Math, Math), i5, Mul@witness(i1, Diff@witness(i0, i2)), Mul@witness(i2, Diff@witness(i0, i1)), i3)` |
| 84 | B73 | `o0 := i0; require i2 ≡ Sin@witness(o0)` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math), ops(o0 := i0, require i2 ≡ Sin@witness(o0)))` |
| 85 | B40 | `o0 := i0; require i1 ≡ Mul@witness(o1, o2)` | [1, 2] | `call:F6(o0 := i0, i1, o1, o2)` |
| 86 | B74 | `o0 := i1; o1 := Diff@witness(i0, i2); o2 := Diff@witness(i0, i2); require Mul@witness(i1, Diff@witness(i0, i2)) ≡ Mul@witness(o0, o1); require Mul@witness(i2, Diff@witness(i0, i1)) ≡ Mul@witness(o0, o2)` | [] | `call:F5(inputs(Math, Math, Math), i1, Diff@witness(i0, i2), Diff@witness(i0, i2), Mul@witness(i1, Diff@witness(i0, i2)), o1, Mul@witness(i2, Diff@witness(i0, i1)), o2)` |
| 87 | B75 | `o0 := i3; o1 := i2; o2 := Diff@witness(i0, i1); require i5 ≡ Mul@witness(o1, o2)` | [] | `call:F3(i3, i2, Diff@witness(i0, i1), i5)` |
| 88 | B75 | `o0 := i3; o1 := i2; o2 := Diff@witness(i0, i1); require i5 ≡ Mul@witness(o1, o2)` | [] | `call:F3(i3, i2, Diff@witness(i0, i1), i5)` |
| 89 | B76 | `o0 := i1; o1 := i0; o2 := Add@witness(i3, i4); require i1 ≡ i2; require i5 ≡ Mul@witness(o0, o1); require i6 ≡ Mul@witness(o0, o2)` | [] | `binding-map(inputs(Math, Math, Math, Math, Math, Math, Math, Math), outputs(Math, Math, Math), ops(o0 := i1, o1 := i0, o2 := Add@witness(i3, i4), require i1 ≡ i2, require i5 ≡ Mul@witness(o0, o1), require i6 ≡ Mul@witness(o0, o2)))` |
| 90 | B77 | `o0 := i1; o1 := i0; o2 := i0; require Mul@witness(i2, i3) ≡ Mul@witness(o0, o1); require Mul@witness(i2, i4) ≡ Mul@witness(o0, o2)` | [] | `call:F5(inputs(Math, Math, Math, Math, Math), i1, i0, i0, Mul@witness(i2, i3), o1, Mul@witness(i2, i4), o2)` |
| 91 | B78 | `o0 := i1; o1 := i0; o2 := i4; require i3 ≡ Mul@witness(o0, o1)` | [] | `call:F0(inputs(Math, Math, Math, Math, Math), i1, i0, i4, i3, o0, o1)` |
| 92 | B56 | `o0 := i1; o1 := i0` | [2] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o0 := i1, o1 := i0))` |
| 93 | B24 | `o0 := i2; o1 := i1; o2 := i0; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F4(i2, i1, i0, i4)` |
| 94 | B79 | `o0 := Mul@witness(i2, i3); o1 := i1; o2 := i0; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F4(Mul@witness(i2, i3), i1, i0, i4)` |
| 95 | B25 | `o0 := i2; o1 := i1; o2 := i0; require i3 ≡ Mul@witness(o1, o2)` | [] | `call:F4(i2, i1, i0, i3)` |
| 96 | B29 | `o1 := i1; o2 := i0` | [0] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o1 := i1, o2 := i0))` |
| 97 | B29 | `o1 := i1; o2 := i0` | [0] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o1 := i1, o2 := i0))` |
| 98 | B80 | `o0 := Integral@witness(i0, i2); o1 := Integral@witness(i1, i2)` | [] | `call:F7(Integral@witness(i0, i2), i1)` |
| 99 | B81 | `o0 := i2; require i1 ≡ Cos@witness(o0)` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math), ops(o0 := i2, require i1 ≡ Cos@witness(o0)))` |
| 100 | B82 | `o0 := i2; require i0 ≡ Mul@witness(o1, o2)` | [1, 2] | `call:F6(o0 := i2, i0, o1, o2)` |
| 101 | B83 | `o0 := i2; require i0 ≡ Cos@witness(o0)` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math), ops(o0 := i2, require i0 ≡ Cos@witness(o0)))` |
| 102 | B84 | `o0 := i2; require i1 ≡ Sin@witness(o0)` | [] | `binding-map(inputs(Math, Math, Math), outputs(Math), ops(o0 := i2, require i1 ≡ Sin@witness(o0)))` |
| 103 | B85 | `o2 := i2; require i1 ≡ Mul@witness(o0, o1)` | [0, 1] | `call:F6(o2 := i2, i1, o0, o1)` |
| 104 | B86 | `o0 := i3; o1 := i0; o2 := Integral@witness(i1, i2); require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F3(i3, i0, Integral@witness(i1, i2), i4)` |
| 105 | B87 | `o0 := i3; o1 := i0; o2 := i1; require i5 ≡ Mul@witness(o1, o2)` | [] | `call:F3(i3, i0, i1, i5)` |
| 106 | B88 | `o0 := i5; o1 := i0; o2 := i1; require i3 ≡ Mul@witness(o1, o2)` | [] | `call:F3(i5, i0, i1, i3)` |
| 107 | B48 | `o0 := i3; o1 := i0; o2 := i1; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F3(i3, i0, i1, i4)` |
| 108 | B89 | `o0 := Mul@witness(i3, i4); o1 := i0; o2 := i1; require i5 ≡ Mul@witness(o1, o2)` | [] | `call:F3(Mul@witness(i3, i4), i0, i1, i5)` |
| 109 | B90 | `o1 := i0; o2 := i1` | [0] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math, Math), ops(o1 := i0, o2 := i1))` |
| 110 | B90 | `o1 := i0; o2 := i1` | [0] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math, Math), ops(o1 := i0, o2 := i1))` |
| 111 | B91 | `o0 := Const@witness(literal:Int(-1)); o1 := Mul@witness(i0, i1); o2 := i2; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F4(Const@witness(literal:Int(-1)), Mul@witness(i0, i1), i2, i4)` |
| 112 | B92 | `o1 := Mul@witness(i0, i1); o2 := i2` | [0] | `binding-map(inputs(Math, Math, Math), outputs(Math, Math, Math), ops(o1 := Mul@witness(i0, i1), o2 := i2))` |
| 113 | B93 | `o1 := i0; o2 := Mul@witness(Const@witness(literal:Int(-1)), i1)` | [0] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o1 := i0, o2 := Mul@witness(Const@witness(literal:Int(-1)), i1)))` |
| 114 | B93 | `o1 := i0; o2 := Mul@witness(Const@witness(literal:Int(-1)), i1)` | [0] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o1 := i0, o2 := Mul@witness(Const@witness(literal:Int(-1)), i1)))` |
| 115 | B94 | `o0 := i2; o1 := Const@witness(literal:Int(-1)); o2 := i1; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F4(i2, Const@witness(literal:Int(-1)), i1, i4)` |
| 116 | B94 | `o0 := i2; o1 := Const@witness(literal:Int(-1)); o2 := i1; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F4(i2, Const@witness(literal:Int(-1)), i1, i4)` |
| 117 | B95 | `o0 := Const@witness(literal:Int(-1)); require i1 ≡ Add@witness(o1, o2)` | [1, 2] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o0 := Const@witness(literal:Int(-1)), require i1 ≡ Add@witness(o1, o2)))` |
| 118 | B96 | `o0 := Const@witness(literal:Int(-1)); require i1 ≡ Mul@witness(o1, o2)` | [1, 2] | `binding-map(inputs(Math, Math), outputs(Math, Math, Math), ops(o0 := Const@witness(literal:Int(-1)), require i1 ≡ Mul@witness(o1, o2)))` |
| 119 | B97 | `o0 := Mul@witness(i0, i1); o1 := Mul@witness(i0, i2); o2 := i5; require Add@witness(o0, o1) ≡ Mul@witness(Diff@witness(i5, i3), Integral@witness(i4, i5))` | [] | `call:F0(inputs(Math, Math, Math, Math, Math, Math), Mul@witness(i0, i1), Mul@witness(i0, i2), i5, Add@witness(o0, o1), Diff@witness(i5, i3), Integral@witness(i4, i5))` |
| 120 | B98 | `o0 := i0; o1 := i1; o2 := i1; require Mul@witness(i0, i1) ≡ Mul@witness(o0, o1); require Mul@witness(i0, i2) ≡ Mul@witness(o0, o2)` | [] | `call:F5(inputs(Math, Math, Math), i0, i1, i1, Mul@witness(i0, i1), o1, Mul@witness(i0, i2), o2)` |
| 121 | B48 | `o0 := i3; o1 := i0; o2 := i1; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F3(i3, i0, i1, i4)` |
| 122 | B21 | `o0 := i4; o1 := i0; o2 := i1; require i3 ≡ Mul@witness(o1, o2)` | [] | `call:F4(i4, i0, i1, i3)` |
| 123 | B48 | `o0 := i3; o1 := i0; o2 := i1; require i4 ≡ Mul@witness(o1, o2)` | [] | `call:F3(i3, i0, i1, i4)` |
| 124 | B99 | `o0 := i0; o1 := i2; o2 := i1; require i3 ≡ Mul@witness(o0, o2); require i4 ≡ Mul@witness(o0, o1)` | [] | `call:F5(inputs(Math, Math, Math, Math, Math), i0, i2, i1, i3, o2, i4, o1)` |

## 同一 wiring 下的不同 rule 组合

- B0：排名 1, 2
  - `[R0@p0:Add#0{head/0/expr/1=>body/0/expr/1}] -> R0`
  - `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1}] -> R1`
- B1：排名 3, 42
  - `[R0@p0:Add#0{head/0/expr/1=>body/0/expr/1/args/1} + R0@p1:Add#1{head/0/expr/1=>body/0/expr/1}] -> R2`
  - `[R0@p0:Add#0{head/0/expr/1=>body/0/expr/1/args/1} + R1@p1:Mul#0{head/0/expr/1=>body/0/expr/1}] -> R9`
- B2：排名 4, 19, 36
  - `[R2@p0:Add#0{head/0/expr/1/args/0=>body/0/expr/1}] -> R0`
  - `[R3@p0:Mul#0{head/0/expr/1/args/0=>body/0/expr/1}] -> R1`
  - `[R9@p0:Mul#0{head/0/expr/1/args/0=>body/0/expr/1}] -> R1`
- B21：排名 24, 122
  - `[R3@p0:Mul#0{head/0/expr/1/args/0=>body/0/expr/1/args/1} + R1@p1:Mul#1{head/0/expr/1=>body/0/expr/1}] -> R3`
  - `[R9@p0:Mul#0{head/0/expr/1/args/0=>body/0/expr/1/args/1} + R1@p1:Mul#1{head/0/expr/1=>body/0/expr/1}] -> R3`
- B24：排名 27, 52, 93
  - `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R14@p1:Diff#0{head/0/expr/1/args/1=>body/0/expr/1}] -> R15`
  - `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R9@p1:Mul#1{head/0/expr/1/args/1=>body/0/expr/1}] -> R3`
  - `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R15@p1:Diff#0{head/0/expr/1/args/0/args/1=>body/0/expr/1}] -> R15`
- B25：排名 28, 50, 95
  - `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R15@p1:Diff#0{head/0/expr/1/args/1/args/1=>body/0/expr/1}] -> R15`
  - `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R3@p1:Mul#1{head/0/expr/1/args/0=>body/0/expr/1}] -> R3`
  - `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R9@p1:Mul#1{head/0/expr/1/args/0=>body/0/expr/1}] -> R3`
- B29：排名 32, 44, 69, 96, 97
  - `[R0@p0:Add#0{head/0/expr/1=>body/0/expr/1/args/1}] -> R2`
  - `[R0@p0:Add#0{head/0/expr/1=>body/0/expr/1/args/1}] -> R14`
  - `[R0@p0:Add#0{head/0/expr/1=>body/0/expr/1/args/1}] -> R9`
  - `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1}] -> R15`
  - `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1}] -> R3`
- B40：排名 46, 64, 85
  - `[R14@p0:Diff#0{head/0/expr/1/args/0=>body/0/expr/1}] -> R15`
  - `[R9@p0:Mul#1{head/0/expr/1/args/0=>body/0/expr/1}] -> R3`
  - `[R15@p0:Diff#0{head/0/expr/1/args/1/args/1=>body/0/expr/1}] -> R15`
- B48：排名 56, 107, 121, 123
  - `[R3@p0:Mul#0{head/0/expr/1/args/0=>body/0/expr/1/args/1} + R15@p1:Diff#0{head/0/expr/1/args/1/args/1=>body/0/expr/1}] -> R15`
  - `[R3@p0:Mul#0{head/0/expr/1/args/0=>body/0/expr/1/args/1} + R3@p1:Mul#1{head/0/expr/1/args/0=>body/0/expr/1}] -> R3`
  - `[R9@p0:Mul#0{head/0/expr/1/args/0=>body/0/expr/1/args/1} + R15@p1:Diff#0{head/0/expr/1/args/1/args/1=>body/0/expr/1}] -> R15`
  - `[R9@p0:Mul#0{head/0/expr/1/args/0=>body/0/expr/1/args/1} + R3@p1:Mul#1{head/0/expr/1/args/0=>body/0/expr/1}] -> R3`
- B50：排名 58, 59
  - `[R3@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R15@p1:Diff#0{head/0/expr/1/args/1/args/1=>body/0/expr/1}] -> R15`
  - `[R3@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R9@p1:Mul#1{head/0/expr/1/args/0=>body/0/expr/1}] -> R3`
- B56：排名 66, 92
  - `[R0@p0:Add#0{head/0/expr/1=>body/0/expr/1/args/0}] -> R21`
  - `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/0}] -> R23`
- B75：排名 87, 88
  - `[R15@p0:Mul#0{head/0/expr/1/args/1=>body/0/expr/1/args/1} + R14@p1:Diff#0{head/0/expr/1/args/1=>body/0/expr/1}] -> R15`
  - `[R15@p0:Mul#0{head/0/expr/1/args/1=>body/0/expr/1/args/1} + R9@p1:Mul#1{head/0/expr/1/args/1=>body/0/expr/1}] -> R3`
- B90：排名 109, 110
  - `[R3@p0:Mul#0{head/0/expr/1/args/0=>body/0/expr/1/args/1}] -> R15`
  - `[R3@p0:Mul#0{head/0/expr/1/args/0=>body/0/expr/1/args/1}] -> R3`
- B93：排名 113, 114
  - `[R4@p0:Add#0{head/0/expr/1=>body/0/expr/1/args/1}] -> R14`
  - `[R4@p0:Add#0{head/0/expr/1=>body/0/expr/1/args/1}] -> R2`
- B94：排名 115, 116
  - `[R4@p0:Mul#0{head/0/expr/1/args/1=>body/0/expr/1/args/1} + R14@p1:Diff#0{head/0/expr/1/args/1=>body/0/expr/1}] -> R15`
  - `[R4@p0:Mul#0{head/0/expr/1/args/1=>body/0/expr/1/args/1} + R9@p1:Mul#1{head/0/expr/1/args/1=>body/0/expr/1}] -> R3`

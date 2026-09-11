# 从生成的 rule comb 中学到的子组合

这里统计压缩后语料的显式库引用，不是新的运行时 rule apply 次数。

## F5

```text
(Lambda (use:R15->R15:Diff p1 0 (path head 0 expr 1 args Var(0) args 1) (path body 0 expr 1)))
```

学习集引用：5；留出集引用：2。

- `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R15@p1:Diff#0{head/0/expr/1/args/1/args/1=>body/0/expr/1}] -> R15`，参数 `['1']`。
- `[R3@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R15@p1:Diff#0{head/0/expr/1/args/1/args/1=>body/0/expr/1}] -> R15`，参数 `['1']`。
- `[R13@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R15@p1:Diff#0{head/0/expr/1/args/0/args/1=>body/0/expr/1}] -> R15`，参数 `['0']`。

## F57

```text
(Lambda (supports (use:R1->R15:Mul p0 0 (path head 0 expr 1) (path body 0 expr 1 args 1)) Var(0)))
```

学习集引用：4；留出集引用：0。

- `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R14@p1:Diff#0{head/0/expr/1/args/1=>body/0/expr/1}] -> R15`，参数 `['(use:R14->R15:Diff p1 0 (path head 0 expr 1 args 1) (path body 0 expr 1))']`。
- `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R15@p1:Diff#0{head/0/expr/1/args/1/args/1=>body/0/expr/1}] -> R15`，参数 `['(Apply Ref(5) 1)']`。
- `[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/1} + R23@p1:Diff#0{head/0/expr/1/args/1/args/0/args/0=>body/0/expr/1}] -> R15`，参数 `['(use:R23->R15:Diff p1 0 (path head 0 expr 1 args 1 args 0 args 0) (path body 0 expr 1))']`。

## F74

```text
(Lambda (supports Var(0) (use:R0->R2:Add p1 1 (path head 0 expr 1) (path body 0 expr 1))))
```

学习集引用：4；留出集引用：1。

- `[R0@p0:Add#0{head/0/expr/1=>body/0/expr/1/args/1} + R0@p1:Add#1{head/0/expr/1=>body/0/expr/1}] -> R2`，参数 `['(use:R0->R2:Add p0 0 (path head 0 expr 1) (path body 0 expr 1 args 1))']`。
- `[R2@p0:Add#0{head/0/expr/1/args/0=>body/0/expr/1/args/1} + R0@p1:Add#1{head/0/expr/1=>body/0/expr/1}] -> R2`，参数 `['(use:R2->R2:Add p0 0 (path head 0 expr 1 args 0) (path body 0 expr 1 args 1))']`。
- `[R9@p0:Add#0{head/0/expr/1=>body/0/expr/1/args/1} + R0@p1:Add#1{head/0/expr/1=>body/0/expr/1}] -> R2`，参数 `['(use:R9->R2:Add p0 0 (path head 0 expr 1) (path body 0 expr 1 args 1))']`。

## F46

```text
(Lambda (supports (use:R15->R1:Mul p0 0 (path head 0 expr 1 args Var(0)) (path body 0 expr 1))))
```

学习集引用：2；留出集引用：0。

- `[R15@p0:Mul#0{head/0/expr/1/args/0=>body/0/expr/1}] -> R1`，参数 `['0']`。
- `[R15@p0:Mul#0{head/0/expr/1/args/1=>body/0/expr/1}] -> R1`，参数 `['1']`。

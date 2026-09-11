# Math microbenchmark 原生运行结果

完整原文件 11 轮，release，无 dependency trace。函数行数逐项匹配仓库快照；合计 1,047,896 行。

单次程序内计时 0.410 秒；macOS time -l 最大 RSS 482,050,048 字节。不是多次基准平均值，也不是有 trace/组合器的耗时。

| 规则 | 原生 num matches |
|---|---:|
| `(rewrite (Add a (Add b c)) (Add (Add a b) c))` | 318575 |
| `(rewrite (Mul a (Add b c)) (Add (Mul a b) (Mul a c)))` | 309894 |
| `(rewrite (Mul a (Mul b c)) (Mul (Mul a b) c))` | 81729 |
| `(rewrite (Add a b) (Add b a))` | 72889 |
| `(rewrite (Mul a b) (Mul b a))` | 53412 |
| `(rewrite (Add (Mul a b) (Mul a c)) (Mul a (Add b c)))` | 44273 |
| `(rewrite (Integral (Add f g) x) (Add (Integral f x) (Integral g x)))` | 24077 |
| `(rewrite (Diff x (Add a b)) (Add (Diff x a) (Diff x b)))` | 15261 |
| `(rewrite (Integral (Mul a b) x) (Sub (Mul a (Integral b x)) (Integral (Mul (Diff...` | 12840 |
| `(rewrite (Diff x (Mul a b)) (Add (Mul a (Diff x b)) (Mul b (Diff x a))))` | 4474 |
| `(rewrite (Sub a b) (Add a (Mul (Const -1) b)))` | 4169 |
| `(rewrite (Integral (Sub f g) x) (Sub (Integral f x) (Integral g x)))` | 2654 |
| `(rewrite (Diff x (Sin x)) (Cos x))` | 2 |
| `(rewrite (Integral (Sin x) x) (Mul (Const -1) (Cos x)))` | 2 |
| `(rewrite (Diff x (Cos x)) (Mul (Const -1) (Sin x)))` | 1 |
| `(rewrite (Integral (Cos x) x) (Sin x))` | 1 |
| `(rewrite (Pow x (Const 2)) (Mul x x))` | 1 |
| `(rewrite (Integral (Const 1) x) x)` | 0 |
| `(rewrite (Mul (Pow a b) (Pow a c)) (Pow a (Add b c)))` | 0 |
| `(rewrite (Mul a (Const 1)) a)` | 0 |
| `(rewrite (Pow x (Const 1)) x)` | 0 |
| `(rewrite (Sub a a) (Const 0))` | 0 |

这些匹配计数来自原生 print-stats，不是已提交变更，也不是 combined rule 使用次数。本轮没有执行全量 dependency tracing 或生成 math 的组合见证文件。

复现：
```sh
CARGO_INCREMENTAL=0 cargo build --release --bin run_egg
/usr/bin/time -l target/release/run_egg egglog/tests/math-microbenchmark.egg experiments/math_microbenchmark/native.json 2> experiments/math_microbenchmark/native.time.txt
python3 experiments/math_microbenchmark/summarize.py
```

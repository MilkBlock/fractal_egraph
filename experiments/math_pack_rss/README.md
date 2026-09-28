# math microbenchmark：虚拟事实存储 vs 原生 egglog 的 RSS 对比

回答"同样的等价节点规模下，egg_layout 相对 egglog 的压缩率是多少"。
测的是**本地真实 egglog runtime**，同一进程内同一张图，packing 前后用 `ps` 读 RSS。
**不是**峰值 RSS，**不是**独立存储原型，也不是规划器替换。

## 结论

**当前没有压缩：在同样的等价节点规模（1,047,896 个 Math e-node / 443,832 个 e-class）下，
egg_layout 的虚拟存储用的 RSS 比原生 egglog 更多，比值（native ÷ egg_layout）为
0.70–1.02，即占用是原生的 1.0×–1.4×；换更丰富的库后恶化到 0.12（8 倍）。**

唯一成立的压缩是**逻辑记账上的局部压缩**：被覆盖的 26.5% 行，descriptor 20.45 MB
对 dense 等价值 24.63 MB，即 **−17.0%**；这既不是 RSS，也不是整图净压缩。

## 实测（同一张图，RSS 单位 MiB）

| 配置 | 虚拟行 | pack 前 | pack 后 | native ÷ egg_layout | 应用耗时 |
|---|---:|---:|---:|---:|---:|
| 原生参考（不加载库） | 0 | 341 | 341 | 1.00 | — |
| match 对照 · round-1 库 | 0 | 343.6 | 425.0 | 0.81 | 0.50 s |
| **pack · round-1 库** | 277,803 (26.5%) | 338.8 | 483.8 | **0.70** | 1.89 s |
| match 对照 · r6 库 | 0 | 341.2 | 2910.9 | 0.12 | 12.1 s |
| **pack · r6 库** | 277,884 (26.5%) | 343.5 | 2791.4 | **0.12** | 21.2 s |

峰值 RSS（`/usr/bin/time -l`，同一 11 轮程序）：原生 **343.3 MiB**；
虚拟模式（第 1 轮后 pack 一次、round-1 库）**338.1 MiB**，比值 1.016。
逐轮增长的 round 11 当前 RSS：原生 333.9 MiB vs 虚拟 331.6 MiB，比值 1.007，
此时 `virtual_rows = 25`（占 0.0024%）。

## 关键发现

1. **RSS 增长不是虚拟描述符本身造成的。** `match` 对照执行完全相同的库片段查询但
   不 pack，RSS 增长几乎一样（r6 库：2.91 GiB vs pack 的 2.79 GiB）。开销在
   `reuse_virtual::visit_library` 的**片段查询与实例物化**路径，不在虚拟行表示。
2. **覆盖会饱和。** round-1 库 7 个模板覆盖 277,803 行；r6 库 23 个模板只多覆盖 81 行
   （277,884）。通用二元模板已经把覆盖率顶到 ~26.5%。
3. **逐轮增长模式下 pack 只在第 1 轮发生**，此时图很小，只虚拟化 25 行，所以
   `math_growth` 的 native/virtual 曲线必然几乎重合——它衡量不了稳态压缩。
   稳态压缩要"跑完再 pack"（本目录的 `math_pack_rss`）。
4. **`analyze --rounds 6` 在 round 5 栈溢出中止**（峰值 1.78 GiB）；此前一次完整
   analyze 在 boundary 9 被外部终止，RSS 6.7 GiB。即"先分析出丰富库再 pack"这条路
   目前跑不通，r6 库是中止前 376 个 cell 中成功 bake 的 95 个入口。

## 复现

```sh
cargo build --release --offline --example math_pack_rss --example math_growth --bin egg_layout

# round-1 库（对完整 math 源做 1 轮 analyze 后 bake）
EGG_LAYOUT_GRAPHS=0 ./target/release/egg_layout analyze \
  --source egglog/tests/math-microbenchmark.egg --rounds 1 --output out/math-lib-full/seed
./target/release/egg_layout reuse-bake out/math-lib-full/library.json out/math-lib-full/seed

# 同一张图，pack 前后 RSS（跑完 11 轮再 pack）
./target/release/examples/math_pack_rss \
  egglog/tests/math-microbenchmark.egg out/math-lib-full/library.json pack
./target/release/examples/math_pack_rss \
  egglog/tests/math-microbenchmark.egg out/math-lib-full/library.json match   # 对照

# 逐轮增长（第 1 轮后 pack 一次）
python3 tools/bench/math_growth.py --library out/math-lib-full/library.json --output out/math-growth-full
```

## 口径边界

- RSS 用 `ps -o rss=` 在 pack 前后各读一次，中间不重建图，所以两侧是同一组
  e-node/e-class；RSS 含分配器保留，**不等于**释放后净占用，也不分离峰值。
- `storage_report` 的 `dense_equivalent_bytes` / `descriptor_bytes` 是逻辑记账，
  不含原生 hash/provenance 索引、tombstone、池容量与分配器开销；模块自身说明
  "not RSS or net compression"。
- `match` 模式是同一批查询但不采用虚拟行的对照，用来分离"查询物化"与"虚拟表示"。
- r6 库来自中止的 analyze，不是完整发现的库；它只用来检验"更丰富的库能否带来压缩"。

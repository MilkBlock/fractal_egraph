# Frozen eggcc Pass 3 workloads

These programs use the same eggcc rules and schedule with different initialization data. Start with `sum-bits`, then use `sum-divisors` for a larger Bril program, `fenwick_tree` for a Rust data structure, and `jacobi-1d` for a PolyBench stencil. `dot-product` provides a useful control: it has more initialization operations than `sum-bits` but requires fewer residual candidate visits in Cell CPU.

Run from the repository root. Give each compile a fresh output directory.

```sh
for case_name in dot-product sum-divisors fenwick_tree jacobi-1d; do
  cargo cell-compile --case "$case_name" --output "out/pass3/$case_name"
  cargo cell-bench --program "out/pass3/$case_name/program.json" --lane baseline --repeat 5
  cargo cell-bench --program "out/pass3/$case_name/program.json" --lane filtered --repeat 5 --verify-stages
done
```

Use `cargo cell-run --program out/pass3/fenwick_tree/program.json --output out/fenwick-run` to export the graph and per-Run report.

## Input provenance

The production capture used eggcc commit [ae80f82](https://github.com/egraphs-good/eggcc/tree/ae80f82a403c1209fb084b115ee89470c7e66d0e) and the Tiger extraction pipeline. The extracted result of each pass fed the following pass. The four new inputs come from the completed capture of 96 passing workloads. Each flat file matched the recorded Pass 3 SHA-256 before splitting. Only its middle fragment is stored per case. The compiler joins it to the existing byte-identical prefix and suffix, verifies the whole-file digest and uses the same frozen Bake training input.

For each new case, [suite.json](suite.json) links the original source at its commit, records the source and captured pre-flattening pass digests, and gives the digest of the concatenated flat program. The production profiler binary SHA-256 was `e08467acacf50309518c9ef8d9de1c3182266a4cc8e17dfa74cf19c62b219def` and the flatten tool SHA-256 was `716bbe870ce88f94903474f7c31507c060a3520392d1c19d5fb6f97940c17d67`. The inputs carry the [eggcc license](LICENSE.eggcc).

## Work and timing

| Case | Initialization lets | Production Pass 3 endpoint nodes | Free Join work, marked | Cell baseline visits | Cell filtered visits | Filtered residual visits | Cell filtered median, ms |
|---|---:|---:|---:|---:|---:|---:|---:|
| sum-bits | 188 | 3,263 | 75,174 | 73,400 | 16,779 | 9,309 | 83.97 |
| dot-product | 364 | 3,347 | 96,901 | 70,680 | 14,700 | 7,889 | 72.08 |
| sum-divisors | 398 | 6,649 | 168,351 | 186,660 | 42,614 | 25,556 | 187.55 |
| fenwick_tree | 648 | 8,875 | 272,691 | 247,829 | 46,043 | 26,522 | 222.97 |
| jacobi-1d | 1,084 | 17,331 | 508,984 | 479,805 | 94,462 | 55,754 | 400.89 |

Initialization lets count temporary bindings in the workload fragment. Endpoint nodes come from the original production pass reports. Free Join work uses the [instrumented baseline tag and counter definitions](../../tools/cell-compiler/benchmarks/free-join/README.md), counting keys, rows and index probes on the exact same marked source as the Cell lanes. Two fresh native runs agreed on each count. The native and Cell counters count different operations.

The Cell CPU measurements used the runtime in `a74737e` on one Apple M3 Max thread. Both lanes used the same program file for each case. Recompiling the sum-bits control reproduced the marked source and all execution plans; only the Bake provenance digest and its enclosing checksums changed. We warmed each lane once and alternated the lane order over seven measured pairs. Timings cover inference only. Candidate counts and final graph digests were stable across repeats, and each pair agreed on the final graph. Stage verification compares Cell baseline and filtered graphs, bindings and scheduling fields. It does not establish equivalence with the production eggcc endpoint.

The range-only extension adds `deriche`, `3mm` and `conjugate-gradient` from the same capture and Bake input. [Expanded range benchmark, partial-domain census and Cargo commands](../../tools/cell-compiler/benchmarks/physical-plans/EXPANDED_RANGE.md)

# Frozen sum-bits and native Free Join

Single thread on an Apple M3 Max with rustc 1.94.0. The Cell lanes reuse the previously measured artifact. Native timings are a separate five-sample run with the counters compiled out. Each native sample starts from a fresh EGraph and times the remaining commands after bootstrap. Cell timings cover schedule execution after constructing the graph. Compilation and final graph export are outside both timers.

| Engine and input | Candidate work counted | Median ms |
| --- | ---: | ---: |
| Native Free Join, original source | 81,912 | 48.96 |
| Native Free Join, marked source | 75,174 | 60.11 |
| Cell CPU baseline, marked | 73,400 | 96.28 |
| Cell CPU shared entry, marked | 20,437 | 86.20 |
| Cell CPU shared entry, Sieve and column intersection, marked | 17,028 | 76.51 |

Cell baseline disables shared entry, Sieve and column intersection. It retains default merge programs. The Cell metric sums direct and residual candidates, merge candidate rows, Spine pivot candidates and shared predicate checks.

| Native input | Enumerated index keys | Projected scan rows | Materialized keys or rows | Index probes, including misses | Bindings delivered to action buffer |
| --- | ---: | ---: | ---: | ---: | ---: |
| Original | 40,425 | 5,918 | 871 | 34,698 | 8,763 |
| Marked | 35,418 | 8,230 | 662 | 30,864 | 9,114 |

The native candidate-work total sums the first four columns. The final column counts completed Free Join bindings before action execution and is excluded from that sum. Scans count rows returned by scan_project, excluding rows rejected inside the table implementation. Index construction, subset refinement, RHS effects and rebuild work are outside this counter. A key can represent several physical rows. The candidate metrics describe work at different operators and do not establish a count of distinct rows or equality of the two engines' graphs. Two fresh native runs reproduced every count. Native timing samples in ms were 62.598, 58.616, 59.678, 60.967 and 60.113 for marked input, interleaved with 49.368, 48.407, 49.527, 48.956 and 48.502 for original input.

## Reproduce

Run in the repository root. The patch applies only to the temporary copy of the immutable baseline tag (62ae6baa5539af4468a8f1a94d6e108de4dcd2df).

```sh
cargo cell-compile --case sum-bits --output out/sum-fj
root="$PWD"
bench="$root/tools/cell-compiler/benchmarks/free-join"
tmp=$(mktemp -d)
git archive egglog-baseline egglog | tar -x -C "$tmp"
(cd "$tmp" && git apply --unidiff-zero "$bench/count.patch")
mkdir -p "$tmp/egglog/examples"
cp "$bench/count_fj.rs" "$tmp/egglog/examples/"
cd "$tmp/egglog"
marked="$root/out/sum-fj/source_sk_mk.egg"
original="$root/out/sum-fj/source.egg"
EGGLOG_COUNT_FREE_JOIN=1 cargo +stable run --locked --release --no-default-features --example count_fj -- "$marked" "$original" "$marked" "$original"
env -u EGGLOG_COUNT_FREE_JOIN cargo +stable run --locked --release --no-default-features --example count_fj -- "$marked" "$original" "$marked" "$original" "$marked" "$original" "$marked" "$original" "$marked" "$original"
```

SHA-256 of the measured original source is `29b8396e56c0a08bc11ca9e5d52afdc25c138e5e4731d2780484bb004b3686d1`. The marked source is `cf5f4c856e6f1c5cd526b81b6d96812a590ba0a28260735823c99bb71e330bb2`.

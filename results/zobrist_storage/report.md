# Zobrist online storage experiment

This is an executable local-eclass storage prototype driven by mutations derived from actual egglog snapshots. It is NOT an integrated replacement of egglog constructor tables, union-find or query indexes. No whole-engine memory reduction is claimed. Scaling repeats source histories with disjoint concrete child IDs; unique is a deliberately synthetic low-reuse negative control.

Raw is a packed sorted u32-row baseline (binary-search deduplication), with per-class fixed stride from the source schema. It retains no structural template or separate child bindings. Shared stores a reference-counted template of u64 tokens plus each instance's concrete u32 bindings. Ports are assigned online on first insertion; removed ports remain bound. There is no exhaustive canonicalization or prototype training.

All modes receive identical insertion, deletion, root-ID-update and redundant-insertion events. Removed/nonexistent and duplicate insertions preserve set semantics. Shared lookup uses (64-bit Zobrist, cardinality) followed by full token-set equality. Dead templates are reclaimed immediately. Shared-rehash recomputes the SAME Zobrist from the entire updated set; shared-zobrist XORs only the changed token. All other storage and exact checks are identical. Detect retains the raw data AND the prototype detector, measuring this concrete sidecar implementation's cost, not a lower bound on detection overhead.

## Measurements

Release build, separate process per run, randomized mode order, 3 repetitions by default. Retained/peak-requested memory counts actual Rust allocation request sizes, including bindings, dictionary, template buckets, vectors and root metadata; excludes common input-reader/process baseline and allocator metadata. Both layouts shrink unused vector capacity at the end. Peak RSS is whole-process /usr/bin/time -l on macOS, including file buffers and allocator effects. RSS numbers are benchmark processes, not egglog processes.

Update time sums timed store mutations; it includes online port assignment, deduplication, allocation, hashing, equality checks and reference reclamation, but excludes input reading, explicit root-ID assignment, validation and final compaction. Full scan directly reads the stored representation without materializing the entire graph. Counter instrumentation overhead is present in every mode. End-to-end stream time, compaction time, scan time and all individual results are in runs.json.

| Workload | classes | final nodes | mode | retained MiB | peak RSS MiB | updates s (median) | scan ms (median) |
|---|---:|---:|---|---:|---:|---:|---:|
| arithmetic | 32768 | 83950 | raw | 1.867 | 4.70 | 0.008 | 0.141 |
| arithmetic | 32768 | 83950 | detect | 2.969 | 7.02 | 0.022 | 0.243 |
| arithmetic | 32768 | 83950 | shared_rehash | 1.258 | 3.83 | 0.016 | 0.100 |
| arithmetic | 32768 | 83950 | shared_zobrist | 1.258 | 3.91 | 0.016 | 0.100 |
| sum4 | 32768 | 786432 | raw | 15.906 | 26.50 | 0.046 | 4.019 |
| sum4 | 32768 | 786432 | detect | 17.157 | 29.70 | 0.181 | 5.059 |
| sum4 | 32768 | 786432 | shared_rehash | 1.407 | 4.09 | 0.161 | 1.099 |
| sum4 | 32768 | 786432 | shared_zobrist | 1.407 | 4.20 | 0.143 | 1.139 |
| sum6 | 1024 | 737280 | raw | 19.716 | 45.97 | 0.088 | 4.112 |
| sum6 | 1024 | 737280 | detect | 19.769 | 44.81 | 0.252 | 5.681 |
| sum6 | 1024 | 737280 | shared_rehash | 0.058 | 2.55 | 0.313 | 1.479 |
| sum6 | 1024 | 737280 | shared_zobrist | 0.058 | 2.05 | 0.166 | 1.492 |
| unique | 16384 | 131072 | raw | 3.953 | 5.73 | 0.009 | 0.729 |
| unique | 16384 | 131072 | detect | 7.984 | 13.16 | 0.070 | 1.059 |
| unique | 16384 | 131072 | shared_rehash | 4.109 | 9.11 | 0.062 | 0.276 |
| unique | 16384 | 131072 | shared_zobrist | 4.109 | 8.61 | 0.058 | 0.281 |

## Correctness and boundaries

7920 exact snapshot comparisons across all modes on unscaled validation streams, including deletions and root-ID changes. Larger runs have identical mutation counts and full-scan checksums across modes; exact collision behavior and copy-on-write are additionally covered by Rust differential tests, including a forced zero-bit hash.

The Sum4/Sum6 rules rotate operands and swap adjacent operands of a commutative sum, and actually saturate in egglog to 24/720 permutations. They intentionally exhibit high repetition and establish a favorable regime, not general compiler-workload performance. Arithmetic uses the prior 16-shape/6-schedule corpus and is a small-eclass counterpoint. Unique has random six-argument patterns and two states per instance; it is not an egglog workload.

The prototype stores concrete child IDs and current root IDs, but external child-class contents, union-find, query indexes, timestamps, subsumption flags and mutation provenance are outside this local membership-store experiment. A fully integrated engine must account for all of them. Stable role bindings can miss isomorphic patterns with different discovery orders; hash equality is never accepted without structural equality.

New templates still require a sorted token vector; incremental hashing does not eliminate that work or equality checking. Any memory benefit comes from sharing templates; the rehash-vs-incremental contrast isolates the additional time benefit of Zobrist updates.

```sh
cargo run --bin zobrist_corpus -- results/zobrist_storage/permutation_snapshots.jsonl
cargo test --lib
cargo build --release --bin zobrist_storage
python3 experiments/zobrist_storage/run.py
```

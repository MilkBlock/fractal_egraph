# Per-table comparison

[Full result](results/README.md) · [All byte fields CSV](results/tables.csv) ·
[Raw measured snapshots](results/tables.json) · [Provenance](results/provenance.json)

This compares every backend table, including `$uf`, on the same 11-round Math graph.
It observes storage capacities and hash-table allocation sizes directly; it does not
estimate table memory by dividing process RSS or multiplying visible row counts alone.

Measurement runs use two exported source copies with a read-only instrumentation
patch. The pristine tag and its clean checkout are unchanged. The patch adds getters
but does not change execution or packing algorithms. It observes invalidated persistent
index storage through exclusive access without refreshing those indexes. Shared binding
and template blocks are reported both as per-table references and as a deduplicated pool.

The current packing probe consumes the exact `fragment_catalog()` emitted by the existing
library packer. Its queries, deduplication, binding construction and virtualization calls
follow that packer. Match counts were checked against the uninstrumented report. The
library object itself is not a backend table and is not charged to a table.

Reproduce (use a new output directory):

```sh
python3 experiments/table_memory/build.py --output out/table-memory-recheck
cargo run --offline --release --example export_table_memory_plans -- \
  out/math-lib-full/library.json out/table-memory-recheck/full-plans.json
cargo run --offline --release --example export_table_memory_plans -- \
  out/math-lib-r6/library.json out/table-memory-recheck/r6-plans.json
out/table-memory-recheck/pristine-probe egglog/tests/math-microbenchmark.egg \
  > out/table-memory-recheck/pristine.json
out/table-memory-recheck/current-probe egglog/tests/math-microbenchmark.egg \
  out/table-memory-recheck/full-plans.json > out/table-memory-recheck/full.json
out/table-memory-recheck/current-probe egglog/tests/math-microbenchmark.egg \
  out/table-memory-recheck/r6-plans.json > out/table-memory-recheck/r6.json
python3 experiments/table_memory/report.py --input out/table-memory-recheck \
  --output out/table-memory-recheck/report
```

The 256-row grouping and current bitmap layout are from the actual current runtime,
not measurement-specific substitutions. Counts and capacities can vary with native
parallel allocation order even when the visible graph is identical; use same-process
before/after snapshots for packing attribution. Detailed exclusions and interpretation
are included in the result. This is not a per-table RSS measurement or a complete heap
profiler, and should not be advertised as one.

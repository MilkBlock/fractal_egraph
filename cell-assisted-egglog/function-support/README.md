# Function reads, guarded updates and native effects

The earlier 3/14 raytrace admission result came from hard rejections in
`egglog/src/cell_assist.rs::prepare_cell_action`, **before analyzer coverage was
consulted**. This change supports the operations used by all 14 rules:

- LHS custom-function rows, with their current values and visibility.
- Table-bound i64 `<`, `>`, `<=`, `>=` residuals, on both CPU and GPU.
- Original native RHS arithmetic, constructors, `set`, custom merges, union and
  panic. Their native evaluation contexts are preserved.

Other computed LHS primitives, other comparator types, globals, containers and
destructive RHS actions remain explicitly unsupported. This is not arbitrary
`.egg` compatibility, a GPU RHS arithmetic engine, or a GPU closure-transition
state machine.

## What actually executes

The device reads the same typed function-row updates as constructor updates.
Changing a function output invalidates dependent Cell proofs and changes fragment
bindings. A previously false guard can become true when that update arrives.
No full native consumer Join is run to produce assisted candidates.

GPU guards compare **signed i64 payloads as two uint32 limbs**, including negative
numbers and adjacent values above 2^53. Native values can be inline or high-bit
intern IDs; they are not numeric cost coordinates and are not dense array sizes.
The equality-parent array uses equality-sort IDs only. A compact GPU hash table
maps encountered i64 value IDs to their immutable bit patterns. A protocol
handshake rejects incompatible worker versions.

A fixed query plan follows connected table parts before disconnected parts; this
is a deterministic binding-dependency order, not a selectivity budget. It avoids
an unnecessary Cartesian prefix for a delta anchored at TCPair whose connecting
ExtractedExpr row appears later in original atom order.

Accepted bindings still execute the original native RHS and merge/rebuild code.
The native sink rechecks only residual predicates at the effect boundary; it
never repeats the relational Join. A false guard is rejected before staging.
The private work item is removed before fallible RHS actions, so an overflow or
panic cannot leave that work item scheduled for future batches. This does not
promise rollback of original RHS effects preceding a native error.

## Real raytrace query conformance

The `:check-assisted` REPL command compiles every rule of the selected original
schedule packet and compares each GPU binding set with a separate full native
query. The actual production-derived checkpoint is the same Pass-3 packet 349
used by the reference benchmark. A second form can advance a native sub-schedule
before checking. Source and binary hashes are retained in `results.json`.

This deliberately tests the query backend with explicit native rows as its
inputs. **It does not fabricate a Cell for each table row, claim analyzer coverage,
or execute the complete 14-rule saturation on the GPU.** RHS compilation occurs;
RHS effects are checked separately by lifecycle tests and the analyzed fixture.
The native full query is an oracle, never a candidate generator or fallback.
These conformance runs are not speed benchmarks.

At the original checkpoint, all **14/14** query binding sets agree exactly,
covering **28,025 complete bindings**. Advancing `terms` once also agrees for all
14 queries (**28,202 bindings**). After the first native `terms` and `terms-helpers` steps, all 14 queries again
agree (**28,373 bindings**), including **158** enabled better-cost updates.
Thus the real snapshot checks include true as well as false update conditions.
Numerical comparison counts can include
lossless output-buffer retry work; they are not distinct native matches.

```sh
swiftc -O cell-assisted-egglog/resident.swift -o out/resident-metal
python3 cell-assisted-egglog/bench_snapshot.py \
  --output out/raytrace-query-check \
  --assisted-worker out/resident-metal

python3 cell-assisted-egglog/bench_snapshot.py --skip-build \
  --output out/raytrace-query-after-first-steps \
  --assisted-worker out/resident-metal \
  --pre-query-run '(seq terms terms-helpers)'
```

No assumptions about Cell coverage are needed for these backend conformance
checks. They exercise function reads, real binding correlations, aliases, cost
payloads and condition filtering on the original large graph.

## The analyzer is a separate limitation

The analyzer can execute/capture guarded function updates and ripen their local
programs. But `src/native_ripen.rs::export_schema` separately refuses to certify
shared closures with primitive guards/actions, or custom merges other than its
supported min/max forms. Their observed saturated states are not promoted into
the certified catalog. This gate was **not removed**: executing a rule correctly
does not establish parameterized closure sharing or transfer conditions.

The checked-in `fixtures/function-update/guarded-source.egg` reproduces this
independent certification limitation. `source.egg` instead uses table reads and
min-merge updates without primitive guards/actions, which the existing analyzer
can certify. Its tiny `analyze` directory is actual analyzer output, not a
handwritten Cell. The runtime uses its Single Cell with fresh held-out values,
including a large i64 cost; Best is an external function fragment.

Native, auxiliary CPU and resident GPU agree on **all 21 logical table rows**,
including 15 Expr constructor nodes in 9 Expr e-classes, and every round's binding
audit is complete. Counts of i64 function outputs are values, not e-classes.
The guarded version's support is independently exercised in the device tests,
without presenting its uncertified closure as reusable.

## Regression coverage

With both workers enabled, 34 tests pass. New checks cover all four comparison
operators on i64::MIN, negative values, zero, adjacent 10^16 values and i64::MAX;
late function updates that enable/disable a match; native Smaller merge effects;
negative-cost panic; checked-add overflow and private worklist cleanup; invalid
guard rejection; and execution using an actual analyzed Cell plus function
updates. Existing union, subsume, visibility, parallel commit and growth tests
remain enabled.

```sh
CELL_RESIDENT_WORKER="$PWD/out/resident-metal" \
CELL_METAL_WORKER="$PWD/out/cell-resident-final/metal-residual" \
RAYON_NUM_THREADS=4 cargo test --offline \
  --manifest-path cell-assisted-egglog/Cargo.toml --target-dir target/cell-assisted
```

Worker variables must point to rebuilt binaries. Missing variables skip the
corresponding GPU integration tests; they were set for the reported run.

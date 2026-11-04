# Three compiler physical plans on frozen Pass 3

The Rust compiler emits a typed `physical_plan` with per-atom range guards, per-rule ordered dedup slots and per-table scalar merge instructions. The loader derives the same information for older artifacts and rejects inconsistent emitted plans. Runtime policy selection is independent for each plan. All three default off.

## Acceptance metrics and results

| Policy | sum-bits median, ms | Faster rounds | Raytrace median, s | Faster rounds |
|---|---:|---:|---:|---:|
| Control | 73.33 | — | 34.82 | — |
| Range guards | 74.51 | 1/7 | 33.27 | 3/3 |
| FD keys | 73.81 | 4/7 | 35.45 | 0/3 |
| Scalar merge | 72.92 | 5/7 | 35.12 | 1/3 |
| All three | 73.56 | 4/7 | 34.41 | 2/3 |

Same-binary, single-threaded Apple M3 Max release runs with the identical frozen artifact per workload. The control uses shared entry nodes, Sieve, column intersection, planned Free Join, first-resumption Spine and static alias receipt forwarding. Each of the five policies had one excluded warm-up. Each measured round uses fresh processes and rotates lane order. Timings cover the entire inference schedule after graph and executor construction. Compilation, plan derivation, export and auditing are outside the timer. With three Raytrace rounds, small time differences should be interpreted alongside work counts and paired results.

The range lane is faster in all three Raytrace pairs and reduces its median by 4.44%. FD keys and scalar merge reduce their respective work counts but show no Raytrace wall-time improvement as standalone policies. Combining all three is slower than range alone on Raytrace. The data support retaining separate policy selection.

### Range guards

**Count** residual and historical candidate visits, input/output domain size, index builds, hits, build rows, update rows, changed index entries, partial-range intersections and maintenance time. Require exact row visibility, unchanged successful bindings and a low-fan-out fallback.

A pure Unit-valued comparison on an i64 column can give an open or closed bound when its other operand is already available. Require a bound prefix column and an existing candidate domain of at least 32 rows. Build the (table, prefix column, range column) index on first demand. Index exact canonical prefix values, not fingerprints. An empty ordered slice rejects the domain. For a selective partial slice, intersect it with the original domain in original order. Broad slices can defer to the ordinary matcher. Original predicate and delta-owner checks remain in force. Post-repair changes remove old keys and insert final visible rows. A version mismatch at lookup drops stale axes.

Raytrace historical visits fall from 9,522,225 to 7,778,113; residual visits fall from 6,522,466 to 4,778,354. The 19,650 admitted domains contain 1,744,112 input references and yield zero output references on this workload. Their equality buckets can be refuted by their extrema. The index is built once, visiting 12,356 rows, then decodes 57,325 updated rows across the schedule. There are 19,649 reuses. Median index maintenance is 57.72 ms, included in inference. Index maintenance rows are recorded separately from the historical candidate counter. A preliminary per-domain cache built 19,650 times and decoded 1,744,112 build rows for the same reduction. It was replaced before the measured matrix. Sum-bits stays below the threshold and builds no index. A separate regression covers partial slices with surviving rows, late updates, deletion and forced fingerprint collisions.

### Order-preserving FD keys

**Count** dedup insert attempts, duplicate count, full and actual key value elements copied, plus inference time. At each original key position, compute the closure of earlier retained slots under complete table keys. Omit an output slot only when it is determined by that prefix for successful bindings in the current immutable snapshot. This preserves both equivalence and lexicographic order; the original full binding, composition preference, witnesses and RHS are retained. The scope is the ordinary per-rule dedup path.

Raytrace copies 11,205,782 key values instead of 18,838,641 across 3,220,586 inserts (38,858 duplicates). Sum-bits copies 28,372 instead of 45,577 across 7,850 inserts (249 duplicates). These figures count value elements, not bytes, allocator calls or avoided candidate rows.

### Scalar merge

**Count** direct comparisons and old projections, unchanged results, generic merge-program calls and operations, function updates and original merge calls. The compiler checks the exact expression and declared scalar type. For direct i64 min/max with arguments old/new in either order, choose the operand with the original result. Pure old projection returns the old operand. Other merges, including ordered callbacks with reads or nested constructions, retain their normal evaluator. The caller still executes the original set or repair and applies its normal change detection.

Raytrace bypasses 2,937,589 generic merge-program calls: 1,015,221 pure old projections (previously zero-operation programs) and 1,922,368 i64 min/max calls. The direct result is the old value 1,832,206 times. Generic program operations fall from 2,028,519 to 106,151; effective function updates remain 1,676,336. Sum-bits bypasses 2,688 program calls and 393 operations. These counts include both RHS and repair merges and do not imply skipped RHS bindings.

## Correctness and Cargo reproduction

Every timed endpoint, per-Run schedule/count digest and committed-effect counter matches the frozen control. Separate untimed release builds hashed the ordered `(rule ID, slot/value map)` sequence before RHS application at each Run. All five policies matched element by element across 1,428 sum-bits and 16,883 Raytrace Runs, including empty Runs; the digest arrays match #23. Raytrace retains 663,403 resumptions and chain depth 29; sum-bits retains 3,034 and depth 12. Full intermediate graphs were additionally checked on sum-bits. Intermediate full Raytrace graph snapshots were outside the audit.

```sh
cargo cell-compile --case sum-bits --emit-free-join-entry --output out/physical-sum
cargo cell-compile --case raytrace --emit-free-join-entry --output out/physical-ray

for pg in out/physical-sum/program.json out/physical-ray/program.json; do
  for modules in stage-spine,static-aliases \
                 stage-spine,static-aliases,range-guards \
                 stage-spine,static-aliases,fd-keys \
                 stage-spine,static-aliases,scalar-merge \
                 stage-spine,static-aliases,range-guards,fd-keys,scalar-merge; do
    cargo cell-bench --program "$pg" --lane free-join-planned --modules "$modules" --repeat 3
  done
done
```

Use `--repeat 1` in rotated fresh processes for the measurement protocol above. `--verify-stages` checks full graph, binding and schedule snapshots on small inputs. The runtime equivalents are `CELL_CPU_RANGE_GUARDS`, `CELL_CPU_FD_KEYS` and `CELL_CPU_SCALAR_MERGE`, each enabled by 1. For the separate ordered-RHS audit, use [the audit patch](../runtime-modules/root-rhs-audit.patch) and the [runtime replay instructions](../runtime-modules/ALIAS_RECEIPTS.md), setting the three flags individually and together. [Raw samples, phase medians, work counters and fingerprints](results.json)

[Range-only transfer to three larger Pass 3 workloads](EXPANDED_RANGE.md) includes mixed-domain counts and an isolated admission-threshold comparison.

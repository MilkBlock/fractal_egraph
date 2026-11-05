# Single-thread attribute-family fusion

The control is the minimal Cell CPU pipeline at `d4074488`. All arms run the complete original schedule on the same guarded 64-block artifacts with Raytrace-trained context admission. The experiment runs in the standalone engine on one CPU thread. The compiler, offline Bake, startup, graph export and audit are outside the inference timer.

## Compiler and execution contract

The checked family plan builds on the existing neighbor and single-write proofs. A member reads the same unary attribute from constructor children and writes that attribute on the constructor result. Eligibility requires numeric min or max merge, equality or i64 input slots, complete register kernels for every table pivot, and no range-plan conflict. RHS copies are normalized to sources. Integer constants, inputs and previous arithmetic results can feed checked add/subtract/multiply or min/max. The attribute table determines family membership. The eligible slot types and full projection order are fixed by the typed recipe. Newly compiled files carry the plan. Loading checks its contents or derives it for older files. Active whole-rule local contexts keep priority.

**Entry.** Consumers of a shared attribute node and watermark may reuse a Run-local ordered packet frontier. Cold frontiers below eight rows take the existing path; cached frontiers are bounded to 65,536 references in total. Reverse Free Join keeps its own smaller domain. Each rule still checks its predicates and gives Spine first claim on each atom/witness. Kernel answers and Spine winners feed the same compact typed lane representation, including the required receipts and direct origin. The lane order matches the original typed input order, including negative integers. Mixed coverage uses stable sorting and deduplication after the Spine winners. The complete-coverage fast return preserves the order already produced by Spine.

**RHS.** Every LHS observes the sealed Run. At the original commit ordinal a lane projects its parent key, evaluates the scalar recipe, reserves the same physical Cell identity and applies the certified merge. A changed value gets an owning frame with its original inputs and computed RHS slots. Every applicable Spine step uses the original begin/capture sequence, including when the proposal leaves the old value in place. Two successive improvements at the same key therefore retain both effects and their respective producers.

**Publication.** After repair, entry scheduling consumes the graph's already coalesced witness headers plus event/forwarding exceptions. The query index updater can supply the repaired final attribute row to both demanded neighbor ports and warm shared entry nodes for their next refresh. Deletions, stale rows and receipts still use the original publication boundary. Shared copies expire at the next publication. The complete publication phase also contains Spine publication, entry notification, range maintenance, query indexes, neighbor updates and Sieve. RHS evaluation, function state and visibility are advanced at their original points.

## Paired measurements

| Pass 3 | Mode | Inference median, ms | CLI median, ms | Paired Δ vs off, ms (faster pairs) |
|---|---|---:|---:|---:|
| sum-bits | Off | 35.648 | 136.142 | — |
| sum-bits | Entry | 35.898 | 135.303 | +0.341 (3/7) |
| sum-bits | Entry + RHS | 35.946 | 135.784 | -0.086 (4/7) |
| sum-bits | Full | 35.400 | 134.927 | -0.407 (5/7) |
| raytrace | Off | 15,303.230 | 17,793.436 | — |
| raytrace | Entry | 15,047.928 | 17,550.020 | -221.260 (3/4) |
| raytrace | Entry + RHS | 14,800.642 | 17,415.371 | -422.644 (3/4) |
| raytrace | Full | 14,781.263 | 17,269.791 | -547.303 (4/4) |

| Pass 3 | Mode | Entry/search, ms (Δ vs off) | RHS, ms (Δ) | Publication, ms (Δ) |
|---|---|---:|---:|---:|
| sum-bits | Off | 15.383 | 8.002 | 5.843 |
| sum-bits | Entry | 15.538 (+0.155) | 7.977 (-0.025) | 5.956 (+0.113) |
| sum-bits | Entry + RHS | 15.409 (+0.026) | 8.224 (+0.222) | 6.113 (+0.270) |
| sum-bits | Full | 15.406 (+0.023) | 7.994 (-0.008) | 5.730 (-0.113) |
| raytrace | Off | 6,660.000 | 3,948.346 | 3,091.373 |
| raytrace | Entry | 6,340.442 (-319.558) | 4,005.336 (+56.990) | 3,105.605 (+14.232) |
| raytrace | Entry + RHS | 6,449.442 (-210.558) | 3,664.550 (-283.796) | 3,084.966 (-6.407) |
| raytrace | Full | 6,381.891 (-278.109) | 3,636.624 (-311.722) | 3,171.374 (+80.001) |

On sum-bits the inference median fell 0.70% relative to off. In the separate rotating comparison, medians for the original minimal binary, off and full were 35.178, 34.409, 36.013 ms.

On Raytrace the inference median fell 3.41% relative to off. In the separate rotating comparison, medians for the original minimal binary, off and full were 16,025.833, 15,331.881, 14,688.235 ms.

Separate instrumented runs, two measured samples per arm following a warm-up. Values are off → full.

| Pass 3 | Family rule matching, ms | Family RHS, ms | Shared family publication callback, ms |
|---|---:|---:|---:|
| sum-bits | 0.659 → 0.708 | 0.251 → 0.184 | 0.017 → 0.023 |
| raytrace | 2,143.717 → 2,055.457 | 1,035.538 → 789.755 | 128.296 → 206.475 |

The four binary variants are cumulative. The off arm uses the ordinary matcher and single-write implementation on this branch; the Run work list is grouped by rule in all four variants. The separate comparison against the original minimal executable exposes the size of this common change. Search includes sealing, stable ordering and deduplication. The three phase columns are independently measured medians from uninstrumented runs. Timed pairs use fresh processes, one excluded warm-up per arm, rotating and reversing arm order, seven measured rounds on sum-bits and four on Raytrace. Individual samples and paired differences are included in the results file.

Nine rules in each artifact met the family restrictions, all propagating `Expr-size`. Raytrace emitted 1,930,012 compact lanes and made 1,118,550 effective family writes. Get, Single and Concat contributed 966,545, 306,775 and 334,264 ordered lanes. The shared frontier was built 1,568 times and reused 18,274 times. The publisher offered 1,126,308 final row receipts to the warm entry cache; an offer counts even when a later Run never reads the cache. Sum-bits emitted 489 lanes and made 144 effective family writes. Candidate visits stayed at 19,154 and 7,263,030 in every arm.

An independent build of the parent commit with only the write audit added reproduced the same ordered write digests. The four audited modes agree byte-for-byte on ordered RHS digests and effective function-write digests across all 1,428 and 16,883 Runs. Each write hashes the epoch, RHS ordinal and rule (or repair), table, physical function index, canonical key and old/new value in commit order. The trace covers 1,449 and 1,676,335 effective inserts/updates after bootstrap. The full final graph, schedule and winning Spine prefix tree also match the frozen minimal references. Resumptions and maximum depth remain 3,034/12 and 663,403/29. A separate renamed min/max fixture exercises two constructors sharing a parent ID, negative arithmetic, deletion/reinsertion and an ineligible multi-write rule.

Raytrace gains most of the measured improvement from the compact entry and write path. The final-row handoff adds publication work and moves some work out of the next search. The total difference between the write and full medians is 19 ms, small relative to between-process variation. The direction on sum-bits differs between the two timing matrices. Fusion remains opt-in.

## Reproduction

Use a fresh output directory for each invocation. Generate the normal frozen program with `cargo cell-compile --case raytrace --output out/ray` or substitute sum-bits. Choose `program.json` from the resulting directory. Set `m=engine/cell-runtime/Cargo.toml` and `program=out/ray/program.json`.

```sh
cargo run --release --locked --manifest-path "$m" -- \
  --program "$program" --output out/off
for stage in attribute-entry attribute-write attribute-fusion; do
  cargo run --release --locked --manifest-path "$m" --features "$stage" -- \
    --program "$program" --output "out/$stage"
done

cargo run --release --locked --manifest-path "$m" --features slot-transfer-audit,spine-shapes -- \
  --program "$program" --output out/check-off
cargo run --release --locked --manifest-path "$m" --features attribute-fusion,slot-transfer-audit,spine-shapes -- \
  --program "$program" --output out/check-fused
for file in rhs writes spine-shapes state; do
  cmp "out/check-off/$file.json" "out/check-fused/$file.json" || exit 1
done
```

Use `CELL_CPU_PROFILE=1` in a separate run for member query time, member RHS time and the shared attribute publication callback. The callback timer covers the subscribed attribute portion only. Use `steps[].phases.query_update` for the full publication cost. [Raw stage measurements and audit digests](attribute-fusion-results.json)

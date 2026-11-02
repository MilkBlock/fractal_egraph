# Scoped Cell refutation: proof slices and integer port bounds

2026-11-03. Same authenticated sum-bits Pass 3 `source_sk_mk` pipeline as
`PRESEARCH_CONTEXT.md`; standalone single-thread Cell CPU, merge programs ON.
No raytrace, native rerun, new Cell library, heuristic candidate limit or budget.

**The proof representation is more useful, but this workload still has no net
speedup. Refutation remains OFF by default.** This extends online negative
evidence at existing marker / checked multi-stage recipe entries. It does not
certify an entire Cell as permanently closed or turn the frozen Analyze output
into a symbolic cross-instance negative-proof library.

## What changed

1. **Observed failure slices.** Record the atoms actually consulted across all
   branches of an unsuccessful query. If no semi-naive exclusion occurred,
   their conjunction alone is unsatisfiable under the current input bindings.
   Drop unconsulted atoms and alpha-normalize variables, preserving constants,
   aliases and correlation. This is a sound observed slice, not a minimum
   unsatisfiable core found by a solver. A reused child proof conservatively
   contributes its current residual conjunction, never another query's slot ids.

   Example: when every `P(x,y)` fails to connect to `R(y,z)`, an unread
   `Tail(z,tag)` is irrelevant. The negative can be reused with a different tag
   while the complete P/R read ranges remain valid. The regression compares all
   six modes and native egglog, not just proof counters.

2. **Actual compiled Cell/marker ports.** The existing checked merge/marker
   consumer program can now supply complete negative evidence. A port with
   candidates that all fail a guard is distinct from a plain empty index.
   All candidate evidence remains in the graph; the proof only suppresses a
   currently impossible query. No new rule or constructor names are hardcoded.

3. **Integer guard implication.** For a one-table port plus an integer comparison,
   a complete negative for `cost > 10` also rejects `cost > 12` on the identical
   port pattern. Preserve every other field constraint and repeated-variable
   connection. Normalize `<`, `<=`, `>`, `>=`, including reversed operands,
   into integer half-lines using i128 bounds, so i64 MIN/MAX cannot overflow.
   This follows the implemented integer primitive semantics; it is not inferred
   from observed numeric samples. A weaker guard is not rejected.

   The ordered bound index groups by full port pattern and compared variable.
   It does not enumerate candidate pairs or delete non-winning candidates. A
   new candidate, union, set, delete or subsume can invalidate its read evidence.

4. **Scoped invalidation.** A table→proof subscription index visits only proofs
   whose tables changed. Exact bucket/table leases then decide validity.
   Unrelated indexes no longer pay global lease maintenance. Expired evidence
   is removed from both proof and bound indexes. Syntactic patterns and numeric
   implications never override a failed lease. Checkpoint restore starts cold.

5. **Allocation before a guard check.** Inspect borrowed operator/argument slices
   before allocating numeric lookup keys. The first implementation copied all
   adjacent query fragments, including those unrelated to numeric guards; this
   extra cost was removed and both attempts are retained in the results.

No RHS, merge result, constructor, union or stage execution is skipped on the
basis of an incomplete negative. Ordinary Run/Repeat/Saturate and publication
remain unchanged. Failed delta partitions are explicitly rejected as proofs.

## Metrics

`refutation_cores_bench.py` reuses the existing pipeline loader, timer and full
stage auditor. It separates four ablations from off:

- `legacy`: previous full-query evidence and global maintenance;
- `scoped`: same proofs, table-scoped maintenance only;
- `cores`: observed slices plus checked merge/marker entry coverage;
- `bounds`: cores plus the integer implication index.

`shadow` performs the original query on a proposed hit and checks it is empty.
Nested checks inside a shadowed query are suppressed to avoid double-crediting
the same row work. Entry checks are not used to stop shadow execution. The
authoritative work saving is still the actual off/on counter difference.

`--profile` records matched delta seeds, residual/compiled read depth, candidate
visits at each depth, rejection positions and each seed's deepest read. Depth
counts table atoms, with the already-matched seed at depth 1. Existing singleton
point projections are outside these read counters; this is not native Free Join
instrumentation. The script validates identical per-rule seed weights and that
the profiled row reduction equals the global candidate-count reduction. It
automatically emits CDFs and per-source-rule/helper work attribution.

`index_only_failures` includes declined unary/lookup-level negatives; it is not
Cell credit. `sliced_proofs` includes proposed slices later declined. Use
`admitted_sliced_proofs` and `admitted_removed_atoms` for accepted reductions.
Historical work attached to a cached proof is not current saved work.

## Final single-run, same-binary results

| Variant | Inference | Search | Candidate visits | Proof hits | Lease checks |
|---|---:|---:|---:|---:|---:|
| Off | 139.152 ms | 73.356 ms | 87,218 | 0 | 0 |
| Legacy | 183.279 ms | 113.008 ms | 87,162 | 33 | 56,406 |
| Scoped maintenance | 180.089 ms | 112.330 ms | 87,162 | 33 | 7,610 |
| Failure slices / ports | 184.277 ms | 115.006 ms | 87,016 | 192 | 11,059 |
| Plus integer bounds | 187.229 ms | 117.925 ms | 87,014 | 194 | 11,041 |

One invocation per variant. Builds, shadow runs, profiles and full audits are
outside these timings. Lightweight integer counters remain in the executor;
optional depth profiling and shadow execution are disabled. There are no means
or best-of selections. All variants preserve 12,930 logical bindings and 4,518
final logical records. Logical matches are not committed mutations.

The final bounds mode saves **204 candidate visits, 0.234% of total**, versus
56 for legacy, but is **34.6% slower than off** and 2.2% slower than legacy.
Scoped maintenance alone reduces lease checks 86.5%; it changes no query
answers or proof hits. This maintenance improvement is not extra Cell knowledge.

The new evidence does generalize:

- 204 accepted proof registrations drop 457 unneeded atoms (these are not the
  same units as the coincidentally equal 204 candidate visits saved);
- 249 registrations describe a nonempty port constrained by a guard;
- 124 hits occur in the checked comparison-entry path;
- 157 hits reuse evidence from an earlier Run; 8 cross ruleset names;
- 8 hits use a genuinely changed integer threshold, but only **2 additional
  candidate visits** disappear compared with cores alone;
- 3,725 unary/lookup-level proposals and 545 delta-incomplete searches receive
  no stored completeness certificate.

There are 25,175 proof probes for 194 hits. The admitted proof set ends with 287
live receipts / 467 read ranges; maintenance is no longer a global sweep, but
constructing and dispatching proof queries still exceeds their small savings.

## Where the work was saved

All variants have **67,807 matched outer seeds**. Inner residual/compiled
candidate visits change from 18,567 to 18,363: **1.10%**. This is a reduction
inside existing searches, not removal of their outer starts. The other 844
global candidate visits lie outside this matched-seed profile and are unchanged.

| Table-read depth | Off rows | Bounds rows | Saved |
|---|---:|---:|---:|
| 2 | 10,068 | 9,937 | 131 |
| 3 | 5,531 | 5,508 | 23 |
| 4 | 1,407 | 1,400 | 7 |
| 5 | 885 | 879 | 6 |
| 6 | 283 | 246 | 37 |
| 7–12 | 393 | 393 | 0 |

Of the 204 saved visits, **124** belong to the two `terms-helpers-helpers`
comparison consumers, **75** to other source consumers, and **5** to generated
structural-key request rules. No marker-publisher savings occur here. The
comparison hits each replace one `TCPair` visit: the marker already supplies
the other side's term/cost. Passthrough saves 38 visits; the Concat/SubTuple
rewrite in always-run saves 16. Deeper row work is unchanged.

This gives a clearer objective than hit rate: **original-consumer row work
avoided per proof construction, validation and dispatch**, together with the
depth distribution. More hits in an already-cheap port read do not establish
value for a larger Bake library.

## Verification and limits

Every non-off final variant passes **1,511 exact stage snapshots**, including all
marker tables, full bindings, visibility, repeat progress and source schedule.
The shadow run retains the off candidate count and checks its predicted
rejections with actual query execution.

27 Python tests pass, including six new tests, the existing 13 refutation tests
and eight context tests containing the 52 semantic fixtures. The 13 refutation
tests also pass explicitly under the bounds variant. Seven Rust tests pass.
New cases compare all variants and native egglog for proof projection,
correlation, weaker/stronger guards, signed extremes, union-added candidates,
forced index fingerprint collisions, hidden ports and cold checkpoint resume.

The previously documented whole-Pass-3 native `TermBop` discrepancy remains;
this establishes off/on preservation, not new native or extraction equivalence.
The frozen input's old RipSat certification receipts are not upgraded. These
are current complete-port / query proofs, not permanently closed symbolic Cell
families. Unknown interfaces still execute the original search.

Reuse remains tied to actual bound port identities. Containment routing still
uses ordered contiguous fragments, not arbitrary subgraph search. A sliced core
with gaps may therefore be harder to rediscover than a stored full conjunction;
proof strength does not guarantee monotonically better hit coverage. The
synthetic projection case works, but general proof routing is unfinished.

## Reproduction

```
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml --bin eggfuse-cell-runtime --example project_stages
python3 -B -m experiments.cell_runtime.refutation_cores_bench --compiled out/known-static-compile --output out/refutation-new --audit
python3 -B -m experiments.cell_runtime.refutation_cores_bench --compiled out/known-static-compile --output out/refutation-profile-new --profile
python3 -B -m experiments.cell_runtime.refutation_cores_bench --compiled out/known-static-compile --output out/refutation-shadow-new --shadow
```

Direct execution uses `CELL_CPU_MARKER_REFUTATION=1`,
`CELL_CPU_REFUTATION_SCOPE=multistage`, and
`CELL_CPU_REFUTATION_VARIANT=legacy|scoped|cores|bounds|shadow`.
`CELL_CPU_REFUTATION_ENTRIES=0` excludes the new compiled port path.
`CELL_CPU_REFUTATION_BOUNDS=0` makes shadow inspect cores without numeric
implication. The ordinary default remains refutation disabled; merge programs
remain enabled. Profiling and measurement fingerprints are recorded separately.

Detailed timing/credit/depth records are in `refutation-core-results/`. The
earlier allocation-heavy attempt is retained, not presented as another sample
of the final version. This small Pass 3 result does not establish that larger
unmarked workloads have the same opportunity distribution.

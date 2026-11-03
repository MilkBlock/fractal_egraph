# Marker-anchored negative certificates: a rejected performance experiment

This is an opt-in change to the **standalone, single-thread Cell CPU engine**.
It does not change native egglog, the marker compiler, or the GPU. Enable with
`CELL_CPU_MARKER_REFUTATION=1`; the default is off. It is kept on an experimental
branch, not promoted into the main workspace or enabled by default.

## What is actually proved

After a real marker has supplied a correlated binding, the executor may cache
an unsuccessful residual join. Admission requires that the search encountered
no semi-naive partition restriction. "No new binding in this delta" is never
treated as "no binding exists". Single remaining table reads are excluded from
this experiment: it targets joins, not credit for a cheap empty lookup.

Each certificate retains the COMPLETE candidate ranges read during that search:

- A bound lookup watches one complete column-index bucket, including the empty
  bucket case. Hash collisions only enlarge that range; exact row checks still
  establish absence.
- Bucket snapshots are shared `Rc<Vec<Dependency>>` objects. Updating any member
  invalidates the snapshot. Even unchanged fingerprints invalidate on a changed
  witness, so collisions cannot conceal a changed exact value.
- An unbound read watches the table version. The marker read is retained too,
  allowing stale evidence to be reclaimed when its source disappears.
- At a published Run boundary, union repair, function updates, insertion and
  visibility changes update indexes before certificates can be reused. Stale
  receipts are discarded. Unrelated changes to another bucket in the same table
  need not invalidate a bound read.

Residual queries are alpha-normalized across consumers, preserving table/type,
primitive, repeated-variable and exact-bound-value distinctions. Different RHSs
may share the same negative query; no effect is inferred or suppressed from a
positive result. Canonical bound identities remain part of the key. This does
**not** generalize one failed instance to all instances of its template.

Only referenced index snapshots are retained, not cloned graphs or pinned Cell
frames. Cache state is not serialized into checkpoints. On resume it starts
empty. The prototype is disabled when Sieve is enabled, because Sieve's separate
proof dependencies are not included in these leases.

**This is runtime-certified negative query reuse, not a Bake proof that an entire
Cell is closed to external interaction.** The existing Bake cannot yet supply
that stronger contract for these real inputs. A canonical eclass ID alone would
not supply it either.

## Real sum-bits Pass 3

Same official compiler artifact as the previous marker-shape experiment with
the Cartesian Get/Get marker removed. All other marker, multistage, field-copy
and takeover settings are fixed. Compositions and Sieve are off in both lanes.
One clean execution per lane, same release binary. Initialization, compile/Bake,
export and audits are outside inference. No raytrace run.

| Metric | Off | On |
|---|---:|---:|
| Inference | 218.190 ms | 225.923 ms |
| Candidate visits | 151,874 | 151,848 |
| Key lookups | 97,113 | 97,103 |
| Search phase | 114.036 ms | 117.277 ms |
| Index update / certificate retirement | 7.154 ms | 11.728 ms |
| Constructor requests | 28,903 | 28,903 |
| Union requests | 4,627 | 4,627 |
| Merge callbacks | 3,473 | 3,473 |
| Repair visits | 13,384 | 13,384 |

The single-run overhead is **3.54%**. There are 437 eligible queries, 249 admitted
negative receipts, 158 searches rejected as delta-incomplete, and 20 positive
searches. Only **10** lookups reuse a negative; they eliminate **26 candidate
visits (0.017%)**. All hits serve the If/Get rule in `state-edge-passthrough`.
There are 144 invalidations and 105 live receipts / 249 read ranges at the end.
The `prior_search_candidates_on_hits` counter describes historical search work;
the measured off/on candidate difference is reported separately above.

The earlier rule-specific key had zero hits (259 admitted negatives). Sharing
alpha-equivalent residuals produces the ten hits but does not justify the
maintenance cost. This is not a large Cell-locality benefit. The raw source's
historical native egglog time is still **52.144 ms**; it was not rerun, and its
known full-graph differences from the Cell executor remain unresolved.

## Verification and conclusion

- **1,622** streamed snapshots (initialization and every Run, including marker
  completion) agree exactly: all tables, values, equality, visibility, full
  binding sequences and schedule coordinates. No auxiliary tables are excluded.
- Eleven focused tests cover cross-instance and cross-rule reuse, preserved
  variable correlation, unrelated bucket updates, late facts, internal union,
  forced zero-bit hash collisions, same-key function updates, subsume/delete,
  delta partition completeness and checkpoint restart. Independent full-LHS
  enumeration is also checked in these fixtures.
- The three existing Rust unit tests also pass.

The prototype is **not enabled**. The hit queries were already cheap, and this
scheme pays certification/expiry costs after the first search. A stronger next
representation would have to certify a template's residual impossibility across
many instantiations, with a complete current boundary contract. Enlarging this
binding-specific cache is not supported by this result. No compiler policy,
heuristic budget, representative selection or match omission was introduced.

## Reproduce

```sh
cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml --bin eggfuse-cell-runtime --example project_stages
EGGFUSE_TEST_FRONTEND=/path/to/eggfuse_frontend python3 -B -m unittest experiments.cell_runtime.refutation_test -v
python3 -B -m experiments.cell_runtime.refutation_bench --pipeline out/marker-shape-sum-bits/remove-compile --output out/NEW --audit
```

The driver reuses the existing verified pipeline loader, timer and metrics.
Compact evidence: `refutation-results/sum-bits.json`. Raw output:
`out/marker-refutation-sum-bits-shared/`; earlier pilot:
`out/marker-refutation-sum-bits-pilot/`. No build or binary trace artifacts are
tracked.

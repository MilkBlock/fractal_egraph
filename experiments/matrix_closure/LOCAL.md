# Local closure with logical publication times

The resident executor now supports relative-address computation blocks and a
fixed ordered cycle of positive rulesets. It computes unpublished conclusions
ahead of their legal Run, then exposes only the requested logical frontier.
It is still a standalone executor for the admitted unary-existence rule family,
not the full egglog runtime or arbitrary effectful Pass-3 rules.

## Replace repeated existence queries with earliest-proof times

For a grounded clause `p1 & ... & pk -> q` scheduled in phase `r`, retain the
earliest logical time `t[q]` at which its result may become visible:

```
ready     = max(t[p1], ..., t[pk])       # 0 for an empty premise list
candidate = next_r(ready)                # strictly later, in phase r
t[q]      = min(t[q], candidate)         # alternative proofs
```

Initially visible inputs have time 0; unproved facts have infinity. For a cycle
of length `m` whose current epoch starts at phase offset `s`:

```
next_r(t) = t + 1 + ((r - 1 - ((s + t) mod m)) mod m)
```

Single-ruleset execution is the special case `m=1`, so `next(t)=t+1`.
All clocks and comparisons are exact integers. The local kernel uses `min/max`
and phase arithmetic, not GEMM or a learned prediction. The original certified
ridge/GEMM and direct GPU controls remain available.

For example, with `A -> B -> C`:

```
A: Seed(x) -> P(x)
A: P(x) and Link(x,y) -> P(y)
B: P(x) and Link(x,y) -> Q(y)
C: Q(x) and Link(x,y) -> R(y)
```

Given `Seed(a), Link(a,b), Link(b,c)`, GPU preparation can discover `P(a)` at
time 1, `Q(b)` at time 2, and `R(c)` at time 3. But `P(b)` cannot appear until
time 4: the first A Run cannot consume the P it produces in that same Run.
Reading after time 1 exposes only `P(a)`. A later ruleset's computation can be
completed early without pretending that its effects were already committed.

Each relaxation extends an actual proof with a strictly later permitted phase.
A positive-time cycle cannot improve a minimum proof, so a minimum proof need
not repeat a fact along a branch. With a finite structural carrier this gives
a finite convergence bound. The implementation's bounds follow from this
property; they are not candidate caps or timeouts. Unseeded cycles remain false.

## GPU representation and execution

- The dynamic compiler still supplies typed clause deltas. GPU kernels build
  the canonical output-to-clause index; CPU does not scan predicate truth.
- Consecutive groups of 64 fact slots form hardware computation blocks. An
  internal dependency is encoded as a relative slot `0..63`; an external port
  is encoded as `64 + canonical_fact_slot`. These are addresses, not new eclass
  identities. The existing typed alias parent mapping remains authoritative.
- One large dispatch runs these blocks as separate 64-thread threadgroups.
  Each group loads a local time vector and closes its local dependencies in
  threadgroup memory, reading frozen external-port times.
- Block results are published into a separate time vector. Subsequent GPU
  passes propagate changes across blocks until the time vector stops changing.
  No original fact tables are materialized during these passes.
- A reveal kernel publishes exactly those previously absent facts with
  `t <= requested_frontier`. Original fact birth times, visibility and logical
  Run update flags remain observable. Later calls can reuse the same forecast.

The current grouping is by slot range, **not** a new claim that every hardware
block is an Analyze RipSatCell. In the initial real snapshot, about 27.2% of
dependency edges are internal to a 64-slot block. Dependency-aware placement is
not implemented; substantial cross-block propagation remains.

Only the structural clause records are allocated in the local mode. Unused
GEMM features, scores and receipts are omitted. Forecast vectors and indices are
separate scratch and never constitute committed facts.

## Dynamic updates and scheduling

An actual interface patch expires the whole interface forecast before the next
Run. Its scratch buffers are released immediately; already committed facts stay
resident. The next forecast begins with only those committed facts as seeds.
This includes new structure, authoritative union, function-row replacement and
explicit subsumption. Irrelevant/no-op changes retain the existing epoch.
The schedule's phase offset survives a mid-cycle patch.

This is currently **interface-wide** forecast invalidation, not optimal per-block
repair. That conservative rule keeps unpublished conclusions from a removed
anchor or overwritten function value from leaking into a later Run.

For one ruleset, existing `step(n)` retains Repeat's early stop at a quiet Run.
For a ruleset cycle, `advance(n)` executes exactly n scheduled phases; a quiet A
does not incorrectly stop a following B. `saturate()` requires a cycle boundary
and stops after a whole quiet cycle. Repeated names in a schedule are allowed.

The multiple-ruleset compiler keeps phase information even for otherwise
identical clauses. It admits the same positive unary-existence class as before.
Its `matrix-scheduled/v1` library schema makes older workers reject it rather
than accidentally interpret phase-tagged bank IDs as single-ruleset arities.
RHS `set`, `union`, `subsume`, arbitrary numeric guards, structural RHS growth
and observable predicate return identities are not silently speculated: they
remain outside this closure program. External edits occur at explicit epoch
boundaries. No general transaction/rollback engine is claimed.

## Verification

- **782 real Raytrace Run boundaries:** exact truth, hidden flags, alias parents
  and birth timestamps match the previously native-verified transcript. The
  complete final graph check is inherited through exact state equality:
  697,922 rows / 524,657 mapped classes. No expected answer is fed to inference.
- **43-command dynamic fixture:** same state transcript, including growth,
  retirement, slot reuse, alias-triggered matches and predicate hiding. Zero
  live contexts release both context and forecast storage.
- **Native A/B/C fixture:** 17 Runs and 30 final rows / 21 mapped classes.
  Tests premature same-Run propagation, a function overwrite before B,
  removal of an anchor whose future result was predicted, union, hidden input,
  phase continuity and whole-cycle saturation.
- **22 unit tests:** include phased equations versus synchronous execution on
  100 deterministic small programs, repeated phases, and existing dynamic tests.

## Performance and remaining costs

One final run per lane, same binary and saved Raytrace inputs, validation off in
timing regions. Original native controls were reused after fingerprint checks.
These are selected-closure replay times including device updates, not full Pass 3:

| Lane | Host replay | GPU command intervals |
|---|---:|---:|
| Certified ridge/GEMM, batched | 139.5 ms | 53.8 ms |
| Direct Boolean, batched | 135.3 ms | 51.8 ms |
| Relative blocks + logical time | **99.1 ms** | **27.9 ms** |

The new lane is 29.0% shorter than GEMM in this pair. The direct control was
101.7 ms in the earlier pair, while its execution logic is unchanged, so do not
claim a stable 26.8% advantage over direct from the final pair. The earlier
local result was 96.7 ms; both runs and revisions are preserved.

There are 16 forecast builds over the 782 logical Runs. In the per-Run audit,
766 calls reuse the prior forecast. The coalesced timing has 36 calls, of which
20 reuse it. GPU cross-block relaxation takes 510 physical passes in total;
the first long epoch uses 390 passes to determine a 532-Run closure frontier.
Execution command buffers decrease from 177 to 158. This is not zero global
synchronization or evidence that the GPU is saturated.

Final local context records: 698,428 bytes. Forecast storage: 1,185,776 bytes.
Metal total allocation: 3,047,424 bytes versus 2,883,584 for the GEMM control;
these are worker allocations, not process RSS or full-graph compression.

CPU wiring still costs **616.0 ms** in this run. Admission + wiring + local replay
is about **715.5 ms**, still above the reused native selected time of 506.3 ms,
before worker setup, production event delivery and table handoff. The expensive
validation-only snapshot adapter remains outside timing. This implementation
does not remove that integration gap or expand coverage to the large effectful
`terms`/`always-run` workloads.

## Entry points

Use the existing build script; it now records `local.metal` in its manifest.
`MATRIX_CLOSURE_MODE=local` selects the new worker path. Python `Resident` accepts
`mode='local'`, and `DynamicContext(core, ['A','B','C'])` compiles a scheduled group.

```sh
MTL_DEBUG_LAYER=1 python-with-numpy experiments/matrix_closure/verify_scheduled.py \
  --frontend FRONTEND --worker WORKER --output out/local-scheduled

# Validate a changed worker without repeating the 88-second snapshot capture.
MTL_DEBUG_LAYER=1 python-with-numpy experiments/matrix_closure/audit_dynamic.py \
  --reference out/matrix-dynamic-perf-input --worker WORKER --output out/local-audit

python-with-numpy experiments/matrix_closure/bench_dynamic.py \
  --reference out/matrix-dynamic-perf-input --frontend FRONTEND --worker WORKER \
  --reuse-native out/matrix-dynamic-perf --candidate-audit out/local-audit/report.json \
  --modes matrix-batched,direct-batched,local-batched --output out/local-timing
```

Sources: `local.metal`, `dynamic_worker.swift`, the existing typed compiler and
delta assembler. Compact provenance and results are in `local-results.json`.

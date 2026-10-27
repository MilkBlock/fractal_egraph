# Resident matrix saturation and batched RHS publication

This is an opt-in execution path in the **actual GPU-owned eggfuse backend**.
It takes native typed rules and real device rows, computes a local fixed point,
then appends real rows to the existing device hashcons tables. It does not replay
a precomputed answer or send bindings to CPU RHS execution.

The first supported class is positive unary predicates over a fixed structural
carrier. Raytrace's complete 17-rule `is-resolved` ruleset qualifies. `terms`,
arbitrary RipSat cells, numeric guards, function updates, subsume and unions
inside the local program do **not** yet qualify. The compiler records the reason
and retains ordinary execution. Analyze-library specialization loading is not
connected to this new macro path.

## Execution contract

```text
CPU: Saturate(R) command + compiler certificate
                   |
GPU: structural rows -> actual marker ports (once)
                   |
     resident fact coordinates Z
       -> gather fixed marker ports X
       -> FP16 input / FP32 accumulation matrix multiply
       -> threshold + exact positive check
       -> update private Z; repeat until fixed point
                   |
     decode newly true coordinates -> typed RHS rows
       -> parallel publication + existing global hashcons index
                   |
CPU: closure status, compact logical iteration counts
```

The marker contains the original rule, source timestamp, dependency coordinates
and output coordinates. It is allocated only for an actual anchor occurrence,
not for every possible row/plan pair. All matching to establish those ports runs
on the GPU. An explicit source marker can itself be an anchor. Ordinary source
constructors are also anchors; this version scans them once per Saturate entry,
and does not yet maintain these markers across entries from commit events.

For example, the real rule

```lisp
(rule ((= lhs (Bop op x y))
       (ExprIsResolved x) (ExprIsResolved y))
      ((ExprIsResolved lhs)) :ruleset is-resolved)
```

becomes an immutable port record for each existing `Bop`, including ports whose
predicate is currently **absent**. Thus the record survives until a child becomes
resolved, without querying `Bop` or joining predicate tables again.

Let `Z[p,id]` be the exact 0/1 presence of predicate `p` on a canonical identity.
Gathering a marker's ports gives `[1,z1,...,zk,0,...]`. A shared coefficient
matrix has one output head for each supported arity. Thresholding the selected
head implements conjunction; scattering/max combines successful outputs:

```text
X = Gather(Z, marker_ports)
Y = X W
A = threshold(Y) AND exact_validity
Z_next = max(Z, ScatterOutputs(A))
```

`W` comes from the closed-form ridge solution on the entire finite Boolean cube,
rounded to FP16 and certified with an FP32 accumulation/threshold error bound.
It is **not a workload-trained prediction**. Positive proposals are always
checked using exact port bits; `EGGFUSE_MATRIX_AUDIT=1` additionally verifies
negative decisions. Uncertifiable widths use the ordinary executor. Node IDs,
types and constructor wiring remain integer payload; they never pass through
floating-point arithmetic.

This is a dense fact representation, not a compressed invertible Spine code.
Matrix dimensions depend on the entry's actual identity extent and marker count;
they are fixed during this closure. Scratch is released afterwards and rebuilt
at the next public boundary. Hidden rows block reinsertion, aliases use rebuilt
canonical identities, repeated-variable and literal anchor constraints remain
exact. No local construction can grow the structural carrier. There are finitely
many predicate coordinates, so each changing iteration adds at least one; the
coordinate count gives a termination/storage bound, not a heuristic cutoff.

## Visibility and verification

- Only an explicit single-ruleset **Saturate** (including singleton sequence
  wrappers and eligible combined rulesets) is replaced. Ordinary Run and finite
  Repeat are unchanged. Prefix diagnostics and resumed schedules use ordinary
  Run execution.
- Private iterations use ping-pong state and retain per-rule seminaive
  watermarks. No rule reads another rule's partial output in the same iteration.
- Global rows remain unchanged until the whole local fixed point is ready.
  Every required new predicate row is then published, with its logical creation
  timestamp. The next externally scheduled consumer sees the complete closure.
- Decode sorts outputs by birth iteration and coordinate. Allocation is stable
  within this implementation, but physical row/ID order may differ from the old
  executor. Graph equality is checked under a typed bijection. This does not
  establish equivalence for arbitrary later order-sensitive merges.
- Capacity, epoch or numeric-certificate failure rejects the private closure
  **before publication** and falls back to ordinary execution. Exhaustion in the
  original executor remains an error, never a truncated successful result.
- Verification compares complete tables, primitive/function values, visibility
  and equality partitions at Saturate exits. Ordinary Run boundaries retain
  their existing checks. The cached native frontend used here omits logical match
  counts; GPU/old-GPU counts are compared separately, and the report explicitly
  marks native match-count checking unavailable.

The macro is not literally one kernel. Dependent matrix steps use kernel
boundaries for global synchronization, encoded eight iterations per command
buffer. The host reads a small convergence block between batches, rather than
rows/bindings or one global table state per iteration. A monolithic kernel would
need a different cross-threadgroup synchronization contract.

## Results on 2026-10-27

Machine: Apple M3 Max. Source SHA256:
`c727cdf724b020ae585c4051fda36b66576c1ce4252a4d9d59fbb7522d0c7d4f`.
Same Swift worker/native frontend, full original schedule, capacity and match
capacity 1,048,576. One timing per lane; no native oracle or validation layer in
the timed runs. Setup and final state export are excluded from inference time.
Detailed fingerprints and checks: [matrix-saturate-results.json](matrix-saturate-results.json).

| Metric | Ordinary device path | Matrix Saturate |
|---|---:|---:|
| Full inference | 44.899 s | 44.161 s |
| `is-resolved` GPU execution | 322.24 ms | 66.40 ms |
| Its GPU preparation | 34.22 ms | 29.70 ms |
| Its GPU matching/propagation | 163.35 ms | 30.58 ms |
| Its GPU publication | 124.67 ms | 6.12 ms |
| Its controller commands | 782 | 36 |
| Newly published predicate rows | 23,230 | 23,230 |

This is **4.85x less GPU time for the supported ruleset**, including its GPU
preparation/publication. All 36 macro calls together took 192.05 ms of host
control/wait time. Tracked private matrix workspace peaked at about **48.9 MiB**,
in addition to the existing ~1.20 GB device allocation. These are allocation
sizes, not process RSS or physical memory compression ratios.

The first full-schedule closure used 28,160 markers, completed 532 local rounds
and published 18,674 rows in one publication. All 10,097 logical Run records
(ruleset, row count, match count, update flag, epoch, union count) agree with the
old device path. Final **699,784 rows / 524,742 eclasses** agree under the exact
typed graph comparison.

Full inference improved only 1.64% in this single comparison. An earlier control
was 41.33 s, demonstrating that these measurements do not establish a stable
whole-Pass speedup. Most time is still in unsupported rulesets. The full device
backend's pre-existing native endpoint mismatch is not repaired by this work.
No new hardware occupancy claim is made for the matrix macro.

Independent native check: use the real Raytrace declarations/initial graph and
17 rules, scheduling initialization then `is-resolved`. Both old and new device
paths match native at the final boundary: 26,158 rows / 26,157 eclasses, 677 Run
records including initialization; the local macro takes 676 rounds. This is an
isolated **real rule/graph scope**, not native equivalence of the entire Pass 3.

16 matrix regression cases pass with Metal validation and exact negative audit:
hidden input/output, cycles, aliases, union between closures, literal anchors,
combined-rule watermarks, all four-port Boolean inputs, deep recursion, finite
Run observers, checkpoint restoration, rejected guards and capacity fallback.
The existing backend suite also passes all 60 checks.

## Independent RHS publication experiment

`--batch-rhs` is a separate opt-in building block. It compacts effective bindings,
evaluates independent constructor recipes in parallel, deduplicates full typed
keys, assigns stable IDs by prefix scan and publishes rows in parallel. Unsafe
actions, dependent constructor chains, overflow and insufficient proposal space
retain ordered execution. It neither predicts validity nor drops effects.

25 regression cases passed. The full Pass-3 experiment admitted 115/222
`always-run` rules and published 147,345 new rows from 183,863 proposals. The
hot Run-5776 RHS fell from 298.83 to 249.11 ms, but full inference went from
41.33 to 42.87 s. Thus it remains **off by default**. Its profile still showed
`batch_open` (the ordered residual) taking ~73% of sampled shader time and mean
kernel occupancy ~3.88%. Those sampled figures describe that earlier batch-RHS
window, not this matrix macro or a full-run average.

Saved evidence: `out/device-batch-publish-comparison-v4.json`,
`out/device-batch-publish-tests-v4-fixed/results.json`,
`out/device-batch-rhs-counters-v4/`. `profile_batch_rhs.py` uses the existing
profile skill and labels indirect dispatch correctly. It must not be used to
claim a matrix-macro profile: the skill's existing replay path only replays Run.

## Reproduce

```sh
python3 experiments/device_backend/build.py \
  --output out/matrix-sat-build --target target/device-backend

EGGFUSE_MATRIX_AUDIT=1 MTL_DEBUG_LAYER=1 \
python3 experiments/device_backend/test_matrix_sat.py \
  --build out/matrix-sat-build --output out/matrix-sat-tests

python3 experiments/device_backend/run.py --source /path/to/pass-3-flat.egg \
  --build out/matrix-sat-build --output out/matrix-sat-pass3 \
  --matrix-saturate --check-every 0 --save-state \
  --capacity 1048576 --match-capacity 1048576
```

For the control, omit `--matrix-saturate` and use a new output directory. Reuse an
unchanged saved control for subsequent analysis. `build.py --worker-only
--reuse-native-build DIR` reuses a fingerprint-checked native frontend without a
Cargo rebuild; this preserves the cached frontend's provenance and capabilities.

```sh
EGGFUSE_MATRIX_AUDIT=1 MTL_DEBUG_LAYER=1 \
python3 experiments/device_backend/verify_matrix_raytrace.py \
  --source /path/to/pass-3-flat.egg --build out/matrix-sat-build \
  --output out/matrix-sat-native

python3 experiments/device_backend/summarize_matrix_sat.py \
  out/matrix-sat-control out/matrix-sat-pass3 \
  --tests out/matrix-sat-tests --native out/matrix-sat-native \
  --output out/matrix-sat-results.json
```

The next semantic extension is the important one: encode `terms` candidate
construction, exact integer cost updates, comparisons and function-merge effects
in this same resident context. More matrices alone do not certify those effects;
nor does the successful predicate closure imply they can be published early.

# Joint execution: coverage and measured fusion experiments

The 12 reported joint calls are the 12 original nested `terms` scopes in the
saved Raytrace Pass-3 schedule. They are not a Cell count. Across those calls,
15,082 root-context visits had a successful binding or a changed comparison
class (out of 226,147 visits). These are neither distinct Cell families nor
committed mutations. The original internal schedule executes 454 `terms`,
1,186 helper and 1,186 comparison phases, producing 2,826 version patches.

The joint certificate still covers only `terms`, `terms-helpers`, and
`terms-helpers-helpers`. **This experiment does not repair that coverage limit.**
`context` creates recursive constructor structure and unions; `always-run`
contains mixed function updates, structural outputs and other effects. They
cannot be admitted by the existing fixed-carrier predicate matrix certificate.
The unified summary now exposes stage runs and serviced contexts separately,
so invocation count is not confused with Cell coverage.

## Actual full Pass-3 on/off experiments

Each pair uses the same frontend and worker, the same generated
`source_sk_mk.egg`, the same executable Cell library, original control tree,
capacity and exact fallback. One run per lane. No native or extraction timing
is included. Both graphs and original/internal schedules match within each pair.
The existing native/device whole-graph difference remains.

| Change | Off | On | Reduction |
|---|---:|---:|---:|
| Device-directed prepare/evaluate + tiled sort tail | 42.1965 s | 41.7401 s | 1.08% |
| Shared encoder inside joint recipe blocks | 42.9709 s | 42.9962 s | -0.06% |
| Shared encoder for ordinary prepare/sort/rebuild | 42.8182 s | 42.7103 s | 0.25% |

These results do **not** establish a substantial or stable speedup. All knobs
remain explicitly opt-in. The earlier unified run was 38.9557 s; both sides of
these newer pairs are slower. That common absolute-time difference has not been
attributed and is not hidden by the paired ratios.

The first experiment removes 1,640 host fences: task counts drive indirect
threadgroups without downloading them before recipe evaluation. It retains a
427,819,008-byte job workspace at the pre-existing match-capacity bound. This is
an explicit memory cost, not compression or proof that fewer tasks were needed.
The sorting comparator and witness order are unchanged; cross-tile distances
remain separate dispatches and each local tail executes in 256-lane shared
memory. In this mode prepare GPU time moves into the match bucket; compare
joint totals rather than claiming a preparation-only speedup.

The second experiment puts 25,756 dispatches into 5,652 compute encoders.
Explicit buffer barriers preserve each causal dependency. The third puts
75,393 ordinary dispatches into 12,910 encoders. This is backend submission
reuse, not additional Cell proof coverage. Dispatches, matches and effects
are not removed by these encoder changes.

## Correctness and visibility

Every phase still reads its original stage input. The original compute/commit
boundary, ordered set/merge, union block processing and stage seal remain.
Prepare/evaluate fusion only removes a host fence within one phase. No stage
is allowed to consume another stage's effects early. Training capture retains
its original pre-compute fence. Capacity errors still stop the exact work and
use the original transaction fallback; no candidate is sampled or dropped.

- Seven loaded-boundary/corruption checks pass, including partial library fallback.
- Sixteen update checks pass: correlated witness bags, late inputs and union,
  multiple spines, large integers, finite Run, sentinel entry and error rollback.
- A 384-root fixture crosses the 256-lane sort boundary and includes empty phases.
  Both variants match native on 7,812 rows / 5,507 classes; off/on schedules and
  complete graphs agree.
- Sixty ordinary backend differential checks pass with shared encoding, including
  custom merges, containers, subsume, primitive failures and congruence rebuild.
- Each full pair matches on 699,784 rows / 524,742 classes. No new claim of
  pristine native equivalence or extraction improvement is made.

## Reproduce

```sh
python3 experiments/device_backend/build.py \
  --output out/device-build-fusion --target out/unused-device-target \
  --worker-only --reuse-native-build out/device-build-scheduled-reviewed

python3 experiments/device_backend/bench_joint_fusion.py \
  --source out/unified-pass3-multistage/source_sk_mk.egg \
  --build out/device-build-fusion --output out/new-fusion-pair \
  --marker out/unified-pass3-multistage/marker-program.json \
  --cell out/unified-pass3-multistage/multi-stage-cell.json \
  --optimization dispatch
```

Use `--optimization encoder` or `backend` for the other isolated pairs.
`all` is available for experiments, but no full combined performance claim is
made here. Environment switches are `EGGFUSE_JOINT_FUSE_PREPARE`,
`EGGFUSE_JOINT_LOCAL_SORT`, `EGGFUSE_JOINT_SHARED_ENCODER` and
`EGGFUSE_SHARED_ENCODER`; all require `1`, and are off by default. The unified
pipeline includes them in verification cache keys. Fusion/sort requires a
fresh device artifact; an incompatible old shader is rejected explicitly.

```sh
python3 experiments/device_backend/test_joint_fusion.py \
  --build out/device-build-fusion --evidence out/cell-update-pass3 \
  --output out/new-wide-fusion-test
EGGFUSE_JOINT_FUSE_PREPARE=1 EGGFUSE_JOINT_LOCAL_SORT=1 \
python3 experiments/device_backend/test_cell_updates.py \
  --build out/device-build-fusion --analyzer target/debug/egg_layout \
  --source /path/to/original/pass-3-flat.egg \
  --evidence-from out/cell-update-pass3 --spines --output out/new-update-test
EGGFUSE_SHARED_ENCODER=1 python3 experiments/device_backend/test_backend.py \
  --build out/device-build-fusion --output out/new-backend-test
```

The next substantial task is a new executable closure contract for growing
structure/function updates outside the terms ABI, then integration through the
same marker and recipe pass. Increasing the reported macro count or grouping
more host calls without such a contract does not solve it. Existing typed
`context`/`always-run` effects must not be erased to force admission.

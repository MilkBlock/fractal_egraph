# Stable invariant selection and real Pass-3 extraction

The CPU control and GPU now produce **the same complete public final graph** on
the captured raytrace Pass-3 workload, including `to-hoist`. Actual production
Tiger extraction also produces the same 29 functions as the original policy.
This is a same-native-fork comparison, not pristine upstream or Cell sat skip.

## Shared selection policy

The original candidate query and `boundary-analysis` position remain in place.
Only `to-hoist`'s merge is replaced by the following extension expression:

```egg
(function to-hoist (Expr Expr) Expr
  :merge (cell-hoist-best old new
           (Expr-size old) (Expr-size new)
           (CellStructuralKey-Expr old)
           (CellStructuralKey-Expr new))
  :unextractable)
```

The Rust extension registers the pure primitive and key sort. This fragment
requires that registration; it is not a standalone upstream `.egg` extension.
Both the original fused native lane and the CPU/GPU assisted lanes use this
identical merge. All merge/rebuild work remains native; no GPU-specific winner
selection is introduced.

1. Prefer the larger **current Expr-size**, preserving the original goal.
2. If sizes tie, compare structural keys, never e-class/row allocation IDs.
3. A key contains typed constructor names, argument positions, primitive values
   and child keys. Contexts are included; variable scopes are not erased.
4. Keys are immutable shared DAGs. Hashing speeds their interning, but the order
   and collision resolution compare full structure. A hash collision test is
   included. Floating zero/NaN keys follow the base sort's equality convention.
5. Input keys are first computed from finite constructor structure, then frozen
   as input anchors. Subsequent keys describe new construction relative to these
   anchors; union merges keys by the same structural minimum. Input anchors are
   **structural values**, not the original numeric IDs renamed into strings.

The auxiliary `cell-stable-keys` ruleset runs immediately before each original
`boundary-analysis`. It updates only private key functions and does not generate
program Exprs. The benchmark includes its complete cost. Initial anchor creation
is in the separately reported shared setup, not hidden as free preparation.

This is an origin-relative deterministic policy for a common input/environment,
not a universal canonical label for arbitrary egraphs. The implementation accepts
the primitive field sorts used by this capture and fails on unsupported container
fields. It needs finite constructor witnesses; arbitrary opaque fresh values and
constructor deletion/recreation are not covered by a general correctness proof.
The current production checkout is unchanged; `stable_choice::setup/schedule`
installs the shared policy in the replay/runtime through normal egglog extension
APIs. No egglog kernel or Metal shader was changed.

## Timing: one release run per lane

Apple M3 Max, one native thread, no query audit or per-match tracing in timed
runs. The entire captured main Pass-3 schedule executes; only the same 14 terms
rules use GPU matching. Input is the saved real Pass-3 input after earlier passes'
extraction. Native flattened schedule semantics apply. This is not a new complete
three-pass frontend/backend build.

| Policy / lane | Main inference (s) | Actual Tiger extraction (s) |
|---|---:|---:|
| Original `merge new`, fused native | 23.781787 | 2.919845 |
| Original `merge new`, CPU same-submit | 24.250096 | 2.783608 |
| Original `merge new`, GPU | 18.021915 | 3.021468 |
| Stable choice, fused native | 24.316070 | 2.642756 |
| **Stable choice, CPU same-submit** | **24.095693** | **2.736535** |
| **Stable choice, GPU** | **17.973416** | **3.051647** |

The stable same-submit CPU/GPU pair passes the full graph gate:

- Inference speedup: **1.3406x**.
- Inference plus Tiger speedup: **1.2762x**.
- GPU setup is 0.070195 s; CPU setup is 0.002650 s. Including these changes the
  inference ratio to about 1.336x.
- Shared parse/input/key-anchor setup is 0.370315 s (original: 0.257387 s).
- Engine clones, JSON serialization/format adaptation, full graph verification
  and output comparison are excluded. These ratios are not complete compiler CLI
  wall-time ratios. Tiger timers include its input parsing, extraction and output.

Stable-key maintenance itself takes **1.185462 s on the GPU lane**, included in
17.973416 s. It executes 449,185 helper matches over 460 helper runs. An initial
implementation repeatedly refined the entire input and spent 21.387736 s on
this work; freezing structural input anchors removes most of that repetition.
That earlier development run is not averaged into this result.

The fused native run rises by 0.534283 s (**2.25%**) in this one paired observation.
The CPU-control and GPU total times happen to be slightly lower than their
original-policy runs; this is not evidence of negative overhead or statistical
significance. The separately measured ~1.19 s of maintenance is real. It is
modest here after optimization, not zero and not guaranteed small on every input.

Original ruleset-run counts, excluding the helper, remain **10,090** in both
assisted lanes, matching the original-policy control. This checks that this
workload did not gain extra ordinary runs through the helper's updated flag.
The auxiliary saturation is not a general proof of preserving early-stop counts
for arbitrary other schedules; such uses require their own boundary checks.

## Final graph and extraction quality

- Stable CPU same-submit vs GPU: **699,069 public rows**, **524,631 mapped
  equality classes**, exact values/equivalence/visibility, including `to-hoist`.
- Expr size: **70,884 e-nodes / 23,659 e-classes** in every lane.
- The 4,922 resolved initial Expr anchors have identical complete TCPair cost
  sets. Another 7,779 are unresolved in both; none are silently counted as good.
- The private key functions contain another **88,596 rows** on the GPU lane;
  these are real additional storage, not a claimed compression gain. RSS was not
  measured in this timing comparison.
- Actual Tiger output contains **29 functions / 8,018 public selected rows**.
  Reconstruction verifies every selected constructor, constant and connection.
  More strongly, all six original/stable outputs are **byte-identical**:

```text
c48fe2b212d341b1c3f3de3ef94e77e3252b661e68be3bf60fbd417bb5ad3ce9
```

Thus the actual extracted IR is unchanged, rather than merely sharing a surrogate
cost. No new executable raytrace/interpreter test is claimed.

**The fused native graph still differs from the sorted-submit graphs** in
materialized extraction intermediates: stable native/GPU have respectively
72,231/73,372 Smaller rows, 63,058/63,038 TCPair rows and 3,627/3,607 TermConcat
rows. The change resolves the diagnosed invariant-choice discrepancy, not that
separate commit-order distinction. Therefore 1.34x is reported against the exact
same-submit CPU control, not as pristine or fused-native full-graph equivalence.
All original/stable fused outputs nevertheless extract to the identical IR above.
The strict graph gate remains enabled; there is no ignored-to-hoist acceptance.

## Reproduce

Use the same current worker and captured input as the earlier Pass-3 benchmark:

```sh
cargo build --offline --release --example raytrace_stable_bench \
  --example tiger_output_check --target-dir target/analyzer

RAYON_NUM_THREADS=1 EGGLOG_PROBE_COUNTS_OFF=1 \
target/analyzer/release/examples/raytrace_stable_bench \
  /Users/mineralsteins/Repos/egg_related/egg_layout/out/raytrace-interactive-20261010/pass-3-flat.egg \
  out/cell-locality-worker/matrix out/raytrace-stable-new

/opt/homebrew/bin/python3 experiments/raytrace_pass3_gpu/extract.py \
  out/raytrace-stable-new
```

Append `--original` to the Rust command, with another fresh output directory, for
the unchanged choice-policy control. It is expected to fail the graph gate but
retain its report and extraction inputs. Pass `--reference-output PATH/native.egg`
to `extract.py` to compare against that policy's actual Tiger output as well.

Exports are unbounded complete graph snapshots, generated only for this explicit
quality check and not tracked in Git. Tiger's production C++ reader requires
pretty/whitespace-delimited JSON; the extraction script enforces this outside the
extraction timer. An initial compact-format attempt failed before extraction and
is excluded. The script removes its own pretty-format temporary inputs afterward.

Relevant implementation: `cell-assisted-egglog/src/stable_choice.rs`,
`examples/raytrace_stable_bench.rs`, `examples/tiger_output_check.rs` and the
existing shared benchmark helper. Reversed initial and fresh allocation, larger
candidate priority, union, cyclic constructors and hash collisions have regression
tests. Saved text results are in `results/stable-choice/`.

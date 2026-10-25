# Resident ridge-driven Cell transfers

This is a **standalone correctness prototype**, not the production Pass-3
runtime and not an acceleration benchmark. It reuses native `whole_cell::Library`
Bake/commit certificates and the unchanged
`experiments/structural_key_memory/train.py::fit` ridge solver.

A resident GPU context can advance through several certified Cell states without
sending intermediate bindings or scores to the CPU. The CPU eventually commits
the recorded macros through the existing native action emitter and independently
checks the complete original facts/equivalence against original-rule saturation.

## What runs where

```text
CPU: native Bake -> validated frames/alias guards/effect recipes
     ridge fit -> FP16 weights + FP32 bias
                          |
                   initial upload
                          v
GPU: exact input/slot frame + integer feature sum + FP16 coordinates
     predict XW + b -> exact guard -> apply slot map + append effect recipe ID
                                  -> incremental coordinate update -> repeat
                          |
                 one final checkpoint
                          v
CPU: native macro commit -> full graph/equivalence verification
```

The resident arrays use Metal private storage. The template, coordinate codes
and weights are persistent read-only buffers. A `RUN` records several kernels
into **one command buffer** and waits once. This is persistent state across
multiple kernels/commands, not a forever-running kernel. There is no CPU read of
intermediate state used to choose the next transfer. Iteration count comes from
the acyclic library's finite depth, not a search timeout or sampled budget.

Source-frame validity is maintained inductively by certified slot maps and a
disjoint fresh-symbol namespace. The GPU checks new boundary aliases rather than
rescanning all old slot pairs. External identity mutation is excluded by this ABI.

The worker accepts a binary protocol: initialize contexts, add external readiness
bits, run a specified number of certified macro steps, and read the final state.
Readiness patches are sparse `(context, added-mask)` pairs. Readiness denotes an
explicit request for the registered graft, supplied by the caller; this prototype
does not discover those requests from a live global match/rebuild stream. A final diagnostic
command independently exposes current coordinates/scores for numerical checking;
its transfer cost is separate from the production-shaped command sequence.

## Compatibility with ridge

The same multi-output solver, regularization, centering/bias and FP16 storage are
used. Its stored weights are scaled by 256, so the device computes
`X @ W / 256 + bias`, exactly as the existing implementation does.

The first attempt used global state/readiness/alias tokens and trained on many
impossible source states. It missed every transfer in this rollout; exact GPU
completion rescued all 82. That failed model is preserved in
`out/resident-ridge-first` and is not described as a working predictor.

The final coordinate program uses the native source-Cell contract:

- Training contexts must satisfy the already-certified source frame.
- Shared elementary features include state, ready ports and input equalities.
- State-conditioned readiness and equality features keep distinct source states
  from diluting each other's responses. Their positions come from the certified
  parameter/interface maps, not hardcoded Add/Mul names.
- No feature directly contains the complete enabled-transition label. Readiness,
  aliases and source state are separate inputs to the learned operator.
- Opaque input IDs are copied through fixed slot maps, never predicted as numbers.

The fixture has 23 symbolic features encoded into the existing 256-coordinate
basis, and three output columns for registered transfers. This small, partly
redundant basis is retained for compatibility; it is not evidence of high tensor
utilization. The training domain has 289 valid state/ready/alias configurations,
45 held out, with 2 threshold errors across their 135 output entries. Fresh numeric
input labels test identity-renaming invariance, not unseen-family generalization.

The GPU stores the **integer sum** of active feature codes. A state change or
readiness patch adds/removes only changed feature contributions; normalization
is updated exactly. It does not traverse the original graph to reconstruct X.
A full CPU coordinate recomputation at the final checkpoint agrees exactly.

A signed permutation of the 256 coordinates and the matching weight transform
was also tested without refitting. It preserves predictions, regularization norm
and all resulting states. This tests a concrete orthogonal transform; arbitrary
floating rotations and lossy low-rank truncation are not certified by it.

## Concrete native Cell chain

`math.egg` and `plan.json` reuse the earlier whole-Cell example:

```text
A = Sat(Add(x,y))
B = Sat(Mul(k, A.root))
C = Sat(Add(B.root, Mul(k,z)))
```

The rule set contains Add/Mul commutativity, distribution and factoring. Native
Bake supplies complete target closures, inherited-slot maps and constructor/union
effect recipes. GPU frames have up to 11 local eclass slots. All intermediate
facts remain represented in the target closure; this is not RHS-only extraction.

Local equality **inside these certified bodies** is represented by shared class
slots. The GPU does not run a general global union-find/hashcons engine. New
constructor/union effects are recorded as certified recipe IDs and instantiated
in native egglog at the checkpoint. Existing inherited classes may not be newly
merged by this ABI; such libraries are rejected. Input partitions outside the
registered specialization are explicitly parked, not guessed or called saturated.

The v1 binary ABI uses u32 handles, a reserved fresh-symbol namespace, and at
most 31 readiness flags; larger interfaces are rejected, not truncated.
The current single-page ABI accepts deterministic acyclic transfers, opaque
Math inputs, stable parameter identities and monotone external readiness. It
rejects branching source states, cyclic transfer graphs, primitive/set/merge
semantics and inherited-class merges. Multi-page branching, cross-context joins,
late external union/canonicalization and automatic specialization Bake/resume
remain outstanding. The native oracle checks each context independently; this
is not a single global eggcc egraph with overlapping Cell instances.

## Verified result

32 contexts run three GPU macro sweeps, receive one sparse readiness update,
then run two more three-sweep batches, including a quiet batch. No intermediate
context is read back. The final states are:

- 25 complete A -> B -> C chains.
- 3 contexts waiting for external readiness.
- 4 contexts explicitly requiring an unregistered alias specialization.
- 82 applied transfers including the valid prefixes of waiting/parked contexts.

| Matrix variant | Applied transfers | Rejected checked proposals | Transfers recovered by exact GPU completion |
|---|---:|---:|---:|
| Learned ridge | 82 | 1 | 0 |
| Deliberately all-negative | 82 | 0 | 82 |
| Deliberately all-positive | 82 | 218 | 0 |
| Ridge after signed coordinate permutation | 82 | 1 | 0 |

All four variants produce identical frames and effect transcripts. Full
constructor/equivalence checks pass for every applied prefix, including separate
A/B endpoints. Named input ports are pinned as well. Unentered contexts contain
only their original opaque inputs. Parked transfers are **not** included in a
claim of completion for the full requested chain.

For each variant, the independent native reference performs 684 original smooth
matches; native certificate-based commit performs zero. That skipping ability
comes from the **existing whole-Cell proof emitter**, not a newly earned GPU or
ridge credit. The new part is resident proposal/guard/frame/coordinate execution
between the input upload and checkpoint. Exact GPU completion also shows that
ridge is not necessary for correctness of this small finite dispatcher. No claim
that it beats a direct exact state machine is made.

The production-shaped Python/Swift binary protocol carries 780 bytes upward and
3,960 bytes downward, including acknowledgments and final symbolic frames/transcripts.
These are IPC byte counts, not measured physical GPU-bus traffic. Immutable
model/template buffer creation happens once at worker startup and is excluded
from these command counts; kernel command descriptors are also not counted. It
uses 53,632 bytes of per-context GPU buffers for 32 contexts, excluding immutable
model/template buffers. These are buffer/transport counts, not RSS compression
ratios. The separate final numeric audit transfers additional data and is recorded
separately. Device durations are diagnostics, not an end-to-end speedup result.

Five focused tests cover proof rejection, unsupported inherited merging,
alias/readiness guards, coordinate transforms, explicit single-macro steps,
quiet replay and late-boundary resumption. The full runner additionally checks
adversarial model outputs, incremental coordinates, original graph equivalence
and zero original smooth queries during native macro commit.

## Reproduce

Use a Python environment containing NumPy (the existing `cell-spine-venv` works).
The runtime checkout is a dependency and is not modified. Builds and binary
packets stay in `out`.

```sh
python3 experiments/resident_ridge/build.py \
  --runtime-repo /Users/mineralsteins/.codex/worktrees/automatic-refutation/egg_layout \
  --output out/resident-ridge-build

python3 experiments/resident_ridge/run.py \
  --build out/resident-ridge-build --output out/resident-ridge-new

python3 -m unittest discover -s experiments/resident_ridge -p 'test_resident.py'
```

GPU execution requires macOS/Metal and the Xcode toolchain used by the existing
worker. `out/resident-ridge-delivery/report.json` contains the validation record;
`results.json` is its compact committed summary. This prototype does not alter
the previous structural-runtime work or enable a new production default.

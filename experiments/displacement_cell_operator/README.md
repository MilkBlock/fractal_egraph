# Deriving a repeatable matrix state from eggcc tuple rules

This is a **manual representation experiment**, not a new Join scheduler and
not a production Cell-assisted runtime. Six unmodified rule bodies are taken
from eggcc `ae80f82a403c1209fb084b115ee89470c7e66d0e`,
`dag_in_context/src/utility/util.egg`. They create `Add-Gets`, descend through
`Concat`, resolve `Single`, and advance `Add-All-Gets` while emitting Get unions.
The fixture supplies type-analysis results (`tuple-length`) as boundary facts.

The experiment asks whether the SAME state representation survives successive
Runs, a late length fact, a newly grafted equivalent Concat, and shared subtrees.
It checks **all** projected AG / ALL / existing Get rows and Get-to-Get and
Get-to-leaf equivalences at each recorded Run, not merely a few positive checks.
Other eggcc rules, extraction, subsumption and their effects are outside scope.

## Three representations, with two useful reductions

### First attempt: displacements as polynomial coefficients

`Add-Gets(origin,node,offset)` normally carries a changing offset in every
binding. A Concat enode with left length `l` supplies two edges:

```
parent -> left   displacement 0
parent -> right  displacement l
```

The native guard `tuple-length(left)` enables **both** edges, not only the right
one. Compile them into a polynomial matrix `T(z)`: each edge contributes `z^d`.
Then a single structural step is polynomial matrix multiplication followed by
nonnegative-support thresholding. Fourier evaluation converts displacement
convolution into a batch of ordinary complex matrix multiplications.

This works on the admitted finite positive-length DAGs, and is checked against
an independent integer set/worklist reference. But it still carries one matrix
per offset frequency and proof-path counts can grow. It is a comparison
representation, not our preferred final form. Residual checks after inverse FFT
are diagnostics, NOT a general proof of exact floating arithmetic. No Tensor
Core performance claim is made for it.

### Second attempt: eliminate the offset axis entirely

Check the finite edge equations:

```
h[child] - h[parent] = edge_displacement
```

If consistent, every path from `r` to `n` has the SAME displacement
`h[n]-h[r]`. Constructor alternatives and different spine paths do not need
different binding coordinates. The offset axis can be removed:

```
before: A[origin, node, offset]
after:  A[origin, node] plus h[node]
```

This is an exact criterion, solved by graph traversal in O(V+E). It is not a
fit to observed offsets. Only enabled rule edges are inspected; a missing
length does not authorize a transition. Separate components may have separate
coordinate origins; joining consistent components rebases coordinates without
changing an already reachable origin-to-node difference.

Algebraically `T(z) = D(z)^-1 T(1) D(z)`, where `D(z)[n,n]=z^h[n]`. All the
frequency operators are the SAME operator in different coordinates. No FFT is
needed after this normalization.

For an ordinary Run, let A be a 0/1 matrix and T the enabled edge matrix:

```
A_next = min(1, A + A T + newly_enabled_diagonal_seeds)
```

This is one GEMM plus an elementwise clamp. Inputs are 0/1, so FP32 accumulation
is exact when a dot product contains fewer than 2^24 terms. Long proof counts
never accumulate: every step clamps back to 0/1. RHS positions are the affine
readout `h[node]-h[origin]`, not newly searched bindings.

### Another state reduction: tasks are prefixes, not arbitrary sets

Within these six rules, every Add-All-Gets chain starts at position 0 and only
adds p+1. For a fixed `(origin,node,offset)`, its positions are a prefix
`{0,...,cursor}`. Store one integer cursor instead of a separate fact per p:

```
advance = present AND length_known AND cursor < length
cursor_next = max(cursor + advance, new_entry ? 0 : -1)
```

Only the current frontier emits a new union; previous edges remain represented
in the deferred effect history. The terminal `p=length` task is retained even
though it emits no next union. A late length resumes the existing cursor.

This prefix invariant would NOT hold for arbitrary externally seeded p values,
holes caused by subsumption, or changing function policies. Those are not
silently admitted. New Get facts feed back into the diagonal AG seeds on the
NEXT Run, preserving the native finite-step boundary.

## Counterexample and exact repair

For `Concat(a,a)`, the same child is at offsets 0 and length(a). The coordinate
equations conflict. Rejecting that conflict is required; unioning the two
positions would lose an output.

The alternate representation splits just the needed `(node,offset)` coordinates.
Their reachability and cursor states then use the same rectangular matrix
program. It constructs an offline occurrence graph, not a full node*offset
allocation. This is exact for the finite fixture, but can still grow to O(V L)
coordinates; there is no universal compression theorem. Topology growth that
adds occurrence coordinates needs repacking. The current occurrence backend
does not yet incrementally implement that repack.

## Closure and grafting

For the admitted DAGs, `K=I+T+...` is a finite path response. Its Boolean support
can be baked by clamped matrix squaring. A newly inserted edge u->v updates
reachability by the outer product:

```
K_next = max(K, K[:,u] K[v,:])
```

The polynomial comparison backend has the corresponding weighted rank-one
update. Both are tested against rebuilding the operator. The response can
answer a covered structural saturation request; **K is not substituted for
one Run**. Nor does a structural K alone certify all eggcc helper rules as
saturated. Numeric frontier progression and native effects remain observable.

## Existing raytrace evidence

`survey.py` reads the saved same-submit CPU Pass-3 canonical table snapshot.
It does not rerun Analyze, alter rules, or retime inference. For every observed
Add-Gets origin it checks its reachable enabled Concat edges, retaining all
constructor alternatives. `survey.json` records the original snapshot hash.

* 6,600 observed origins; 886 have a nontrivial Concat dependency.
* 824/886 (93.0%) admit a path-independent offset in that local scope.
* 62 fail, matching the 62 origins with multiple observed offsets for a node.
* Across the 18,641 observed AG rows, 74 are additional offsets beyond one per
  `(origin,node)`; the largest observed multiplicity is three. This is only a
  final-snapshot statistic, not a future bound or a global coordinate count.
* 774 origins additionally meet this experiment's positive-length DAG replay
  contract. The three largest replayed slices have 94, 90 and 74 tuple nodes.

The replay preserves their Concat alternatives, Single leaves and lengths;
opaque leaves are boundary placeholders. It runs the six real rules afresh,
not the full Pass 3. These are manually isolated closure fragments, not claims
of automatic Analyze Cell certification.

For the 94-node fragment, the one-step edge matrix is 3.98% occupied; the baked
closure is 22.45%; the node-to-leaf response is 29.97%. Restricting the latter to
roots with at least half the fragment's maximum length yields 69.38% occupancy.
That last number is an explicitly restricted block, NOT the density of the
whole program. The full offset axis had 24 positions; the normalized iterative
state no longer carries it.

The largest path multiplicity is 37,506, but native egglog does not enumerate
these proof paths either. That number explains why path counting is a bad
representation; it is NOT a count of saved native joins. Byte counts in the
result are prototype array payloads, not process RSS or egraph compression.

## GPU path and remaining limitations

`gpu.swift` executes the coordinate recurrence with MPS GEMM, masks, cursor
updates, affine output indices and atomic OR for existing-Get visibility.
Both ordinary coordinates and split occurrence coordinates use the same
rectangular program. The state stays resident through all test rounds in one
command buffer. Epoch tables contain only changed topology/length metadata;
there is no per-round CPU binding search or state readback. Diagnostic copies
of each round are read at the end and compared with the native-verified oracle.

Union requests are deferred and decoded; this worker does NOT perform general
union-find/rebuild, arbitrary constructor discovery, or production submissions.
An external union that changes the tuple dependency topology must invalidate
or update this representation. The staged graft test is a known topology event,
not an automatic global event-to-Cell detector. Rule-environment changes,
nonmonotone updates, zero-length cycles, capacity growth and unrelated primitive
operations need separate contracts.

GPU validation is on Apple Metal/MPS. It establishes a fixed GPU dataflow, not
NVIDIA Tensor Core use, 90% hardware utilization, or an end-to-end speedup.
No performance timing is taken; matrix bake, context upkeep, emission and CPU
commit costs have not been compared with native execution. A dense matrix is
not by itself evidence that GEMM is faster than sparse propagation.

Verified on Apple M3 Max: **six cases, 111 recorded Run boundaries**, including
the three actual raytrace constructor slices, late length, a new equivalent
spine, and a repeated subtree. All GPU A/cursor/Get states and union-emission
masks equal the independently native-verified coordinate reference. Seven
regressions also pass, including 384 randomized-but-reproducible integer-reference
Run boundaries with missing lengths and late roots. `results.json` retains the
per-round counts and binary identities. GPU diagnostic state copies are not a
production transport design; they are retained only to verify each boundary.

## Reproduce

Use Python with NumPy. `NATIVE` below is the existing
`target/.../release/examples/native_math_baseline` binary (or build that existing
example). The script records its hash. No Rust kernel changes are required.

```sh
python -m unittest discover -s experiments/displacement_cell_operator -p 'test_*.py' -v
python experiments/displacement_cell_operator/survey.py \
  --snapshot PATH/graphs/cpu.control.json --output out/coordinate-survey.json

# Use matching compiler and SDK; a stale local CLT installation may fail with
# duplicate SwiftBridging modules. Do not modify the system module maps.
/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
  -O -sdk /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
  experiments/displacement_cell_operator/gpu.swift -o out/coordinate-worker

python experiments/displacement_cell_operator/run.py \
  --native "$NATIVE" --survey out/coordinate-survey.json \
  --gpu out/coordinate-worker --output out/coordinate-verified
```

Omit `--gpu` for the CPU matrix/reference/native checks. Omit `--survey` for
the standalone late-fact, graft and repeated-subtree fixtures. Output directories
must be new, preserving prior evidence. Generated snapshots and binaries stay
under ignored `out/`; only sources and compact reports are committed.

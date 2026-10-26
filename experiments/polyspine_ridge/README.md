# Polyspine ridge on Raytrace Pass 3

Polyspine means overlapping matching-spine views under successive ruleset
saturations. This first experiment asks whether **pre-stage, connected
interfaces from other stages** improve a ridge predictor over ordinary local
structure. It does not replace the runtime, prove a RipSat closure, discover
all Cells, or claim an end-to-end speedup.

Measured outcome and limitations: [RESULTS.md](RESULTS.md). The initial encoding
only slightly improves total purity classification and worsens newly enabled
purity recall. It is not ready to authorize skipping matches.

Follow-up: [residual context](RESIDUAL.md) restores already-committed entry
facts, separates sparse nonlinear propagation from ridge credit, and tests
explicit numeric state columns. See [RESIDUAL_RESULTS.md](RESIDUAL_RESULTS.md)
for the measured gain and the remaining structural/union cases.

## Native observations and causal boundary

`study.py collect` uses the existing native parser/typechecker/oracle and the
saved complete Raytrace Pass-3 Run trace. It reconstructs actual `Saturate`
boundaries from the schedule AST and checks every replayed update flag. No
extra saturation or changed rule is inserted.

We observe the first invocation of four families in each outer optimization
cycle: `type-analysis` (with its nested helpers), `always-run` (with nested
helpers), `is-resolved`, and the joint `terms`/helper saturation. All intervening
Runs execute natively. Selection uses the schedule, not the resulting labels.
Every pre-existing Expr coordinate is retained, including negatives.

For each stage, the label is existence of an answer **after that stage**:

| Stage | Projected target |
|---|---|
| type-analysis | HasType(expr, ...) |
| always-run | ExprIsPure(expr) |
| is-resolved | ExprIsResolved(expr) |
| terms | ExtractedExpr(expr) has a value |

This is projected answer-existence prediction, not a complete binding, a cost
prediction, a proof, or a count of committed mutations. New Expr coordinates
created during the stage are outside the pre-stage prediction domain.

All feature bags are frozen before executing the stage. The target table and
the active stage's generated analysis outputs are masked even if already
present at entry. Native relations' private result sorts are not structural
features. Original typed IDs are used solely to establish actual connectivity;
their numbers are never regression features. Post-stage union mappings are
used only to find the label of a pre-stage coordinate.

## Matched ablations

Every model uses 512 coordinates (two 256-coordinate blocks), the same fixed
ridge penalty 1, FP16 weights, FP32 scores and threshold 0.5. No hyperparameter
search is performed. The first six outer cycles train; the last six test.
This is future-stage testing in one program; bindings may recur. It is not an
unseen-program result. The token encoder is fixed, not learned from test labels.

1. **structure**: ordinary constructor heads, ordered one-edge paths, incoming
   roles and aliases; second block empty.
2. **stage**: add current and previous observed stage labels. Each target has
   its own head already, so current-stage identity is redundant; this control
   must not be interpreted as a general rejection of stage-aware models.
3. **interfaces**: add committed, locally incident interface rows that are read
   by the current family and can be supplied outside it. Preserve argument
   position, declared type, enum heads and within-row alias conditions.
4. **polyspine**: additionally connect those interfaces through actual ordered
   constructor edges, in both directions. An interface at the left child is
   different from one at the right child or at an unrelated root. More than
   one distinct contributing coordinate is explicitly represented; the source
   count feature uses exact 2/3 and a declared 4-or-more bucket. This is a lossy
   feature, not a work limit or an exact binding-count guarantee.
5. **shuffled_seams**: preserve each frame's cross-interface bags but randomly
   reassign them to different roots, keeping local features unchanged. The
   seed is fixed independently of labels. This checks connection-specific
   information rather than simply supplying more coordinates.

The source-stage tags are the **possible writer rulesets** in the typed rule
program. They are not per-row rule-application provenance. The features capture
an observed overlap view, not a certified whole-Spine partition. Current-stage
interfaces outside the admitted read set are not explored in this experiment.

One example is an `always-run` Bop with a type-analysis interface on its child:

```
local:       Bop at root; role 1 is an Expr
interface:   HasType at the root, if present
polyspine:   Bop.role[1] is connected to HasType.argument[0]
```

Moving that HasType row to a disconnected Expr leaves the root-only summaries
unchanged but removes the Polyspine seam. Repeated-variable aliases are retained.
These are approximate coordinates, never equality certificates.

## Accounting and GPU

Equal feature signatures share regression work without erasing contradictory
labels. Reports include precision/recall, false positives/negatives, raw ridge
Brier error, the remaining contradictory-signature population and the subset
without a pre-existing target answer. The latter prevents already-established
answers from dominating the reported benefit. Paired fixed/broken examples
compare the same test rows; signature-unseen results are separate.

The existing `superposed_key_memory/gpu.swift` reads SPINE003 packets, generates
dense coordinates on the Apple GPU and executes simdgroup matrix multiplication.
All five models use this same worker. Its score reference is FP16-weight CPU
arithmetic, not target truth. It emits all four heads; unused head scores are
reported, not credited as useful inference. This is not NVIDIA Tensor Core
utilization evidence.

Native snapshot delivery, CPU feature extraction/packing, offline fitting and
GPU evaluation are reported separately. A fast GEMM does not erase extraction
cost. No production event publisher, matrix-resident Polyspine maintenance or
table writeback is implemented here. Predicted negatives are never used to
discard native work; a deployed version still needs exact completion.

## Reproduce and reuse

Use the existing NumPy environment. Capture is expensive and should be reused;
changing regression controls does not require replaying the native program.
The input source and native binary must match the verified trace fingerprints.

```sh
python experiments/polyspine_ridge/study.py collect \
  --workload out/matrix-dynamic-perf-input/workload.json \
  --frontend out/matrix-dynamic-bench-build/frontend \
  --output out/polyspine-pass3-data

mkdir -p out/polyspine-ridge-build
xcrun swiftc -O experiments/superposed_key_memory/gpu.swift \
  -o out/polyspine-ridge-build/worker

python experiments/polyspine_ridge/study.py fit \
  --dataset out/polyspine-pass3-data \
  --worker out/polyspine-ridge-build/worker \
  --output out/polyspine-pass3-ridge

python -m unittest discover -s experiments/polyspine_ridge -p 'test_*.py'

# Regenerate the report without rerunning native inference or GPU evaluation:
python experiments/polyspine_ridge/report.py \
  --result out/polyspine-pass3-ridge/results.json \
  --output experiments/polyspine_ridge/RESULTS.md
```

Datasets, binary GPU packets and compiled workers remain untracked. Commit only
the experiment sources and compact text results. Existing engine and previous
Spine-executor changes are intentionally untouched.

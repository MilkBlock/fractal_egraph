# Isolate source-marker effects from backend effects

The cell-assisted executable now runs a complete `.egg` in `program` mode:

- `pristine`: launches a verified runner built against the unchanged
  `egglog-baseline` kernel tree. The baseline checkout and tag are untouched.
- `gpu`: uses the current full GPU matcher and native RHS/merge/rebuild.
- `native`: ordinary CPU execution of the current fork, clearly distinguished
  from pristine upstream.

All lanes use the exact input file, original source policy, native initialization
before the final/main schedule, and retained-checkpoint/clone execution. No
stable-choice rewrite, Cell observer, Bake, marker insertion or schedule rewrite
is silently enabled. GPU mode executes the main schedule; following checks,
extractions and other commands run natively from their original AST. In this
mode, source transformations and backend selection are independent choices.

## One command

```sh
/opt/homebrew/bin/python3 skills/cell-assisted-egglog-bench/scripts/bench.py program-modes \
  --source original.egg --marked with-markers.egg --backend both \
  --worker out/commit-marker-worker/matrix --output out/marker-comparison
```

Omit `--marked` for a same-source backend check. `--backend pristine` or `gpu`
runs only that backend. First use builds the public-API pristine harness offline;
`--no-build` thereafter requires the verified binaries. `--query-audit` enables
the native per-Run binding oracle for GPU, outside performance measurements.
Default paths can be overridden with `--pristine-checkout` and `--worker`.

Individual execution also works without the skill wrapper:

```sh
cargo run --release --manifest-path cell-assisted-egglog/Cargo.toml -- \
  --mode program --backend gpu --source with-markers.egg \
  --gpu-worker out/commit-marker-worker/matrix --output out/one-gpu

cargo run --release --manifest-path cell-assisted-egglog/Cargo.toml -- \
  --mode program --backend pristine --source with-markers.egg \
  --pristine-runner out/program-modes/pristine --output out/one-pristine
```

The wrapper compares four edges separately:

| Check | Compared results |
|---|---|
| `marker_effect_pristine` | original vs marked, both pristine |
| `marker_effect_gpu` | original vs marked, both GPU |
| `backend_original` | pristine vs GPU on original source |
| `backend_marked` | pristine vs GPU on marked source |

All original public table schemas, rows, base/function values, equality partitions
and visibility flags are checked modulo e-class IDs. Marker comparisons project
out only extra tables absent from the original source, and list them explicitly.
Same-source backend comparisons include auxiliary tables too. Unknown container
value encodings or an ungrounded cycle produce **inconclusive**, not equality.
This is an observed final-state check, not a proof of equivalence under all inputs
or a requirement that nonconfluent engine orders produce identical results.

Each source/backend lane is cached by source, actual executor, worker, harness
and protocol. Changing the GPU launcher does not invalidate an unchanged
pristine child executable. Failed execution is retained and never counted as
successful cache reuse. Comparison failures return nonzero and remain visible.

## Validation

1. `original.egg` vs `marked.egg`: all four comparisons pass, with native GPU
   binding audit enabled. The marker has real rows and real consumers.
2. `bad-marker.egg` intentionally omits the B condition. Both source comparisons
   fail, while both same-source backend comparisons pass. The original lanes
   are reused from (1), rather than executed again.
3. CLI regressions cover ordinary native execution, a failed user check, a
   forged pristine binary fingerprint, and GPU execution followed by native
   positive/negative checks.
4. Six graph-comparator tests cover renamed IDs, function updates, union,
   subsume, auxiliary projection, and an inconclusive ungrounded cycle.

## Actual complete raytrace Pass 3

Input: the existing `pass-3-flat.egg`; marker source: the previously generated
and fingerprinted `pass3-source-markers-final/published.egg`. These preserve the
original `to-hoist :merge new`; unlike the earlier stable-choice benchmark,
this experiment does not rewrite it.

| Check | Result |
|---|---|
| Original vs marker on pristine | **different at `to-hoist` equality mapping** |
| Original vs marker on GPU | **equal**, 699,068 original public rows |
| Pristine vs GPU, original source | **different** |
| Pristine vs GPU, marker source | **different** |

The backend difference already exists without marker insertion. Its differing
row counts include Smaller (72,255 vs 73,371), TCPair (63,060 vs 63,038), and
TermConcat (3,629 vs 3,607). The source-marker difference on pristine does not
require a GPU. The order-sensitive `to-hoist` choice is relevant, but this report
does not claim a complete causal proof or silently fix the source policy.

All four programs execute successfully and export complete snapshots. The
comparison command correctly returns nonzero; no pristine-equivalence or
speedup claim is made. No new Tiger quality comparison was run in this isolation
experiment. Repeating the command with a fresh report directory reused **all
four execution lanes** and only recomputed comparisons.

New auxiliary tables returning an original non-primitive sort are not silently
projected away: they may introduce new extraction alternatives. Such a comparison
requires an explicit auxiliary/extraction contract and currently reports inconclusive.

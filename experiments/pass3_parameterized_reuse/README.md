# Parameterized queries on real raytrace Pass 3

This experiment runs the **complete saved raytrace Pass-3 input and schedule**.
Native egglog still commits every original RHS, function merge and rebuild;
GPU executes the remaining queries. It is not a toy or a pristine-egglog speedup.

Seven additional source-derived query programs reuse fixed binding wiring and
committed-row subscriptions. They are an executable step toward parameterized
Cells, **not automatic RipSat family discovery, nor a proof that an entire
recursive closure can be instantiated in one operation**. All finite Run
frontiers remain unchanged. Nothing is enabled by default.

## Programs tested

All variants retain the existing terms/cost proofs and the original 84-root
DoWhile context program. Neither receives new credit here.

| File | Additional programs |
|---|---|
| `baseline.json` | None |
| `tuple.json` | Five Get / Add-Gets / Add-All-Gets queries |
| `if.json` | Two If state-passthrough queries |
| `combined.json` | Both groups |

The tuple group follows Concat/Single structure, offsets and lengths. Its last
rule is the original rule below; actions and arithmetic remain native:

```egg
(rule ((Add-All-Gets orig something offset pos)
       (< pos (tuple-length something)))
      ((union (Get orig (+ offset pos)) (Get something pos))
       (Add-All-Gets orig something offset (+ pos 1)))
      :ruleset always-run)
```

The compiled query reads the current length by the already-bound `something`,
checks `pos`, and returns the original complete binding. The successor position
becomes eligible at its original subsequent Run. It does **not** jump to the end
of the interval or omit intermediate Add-All-Gets/Get rows.

The two If programs retain the correlated `i,j` bindings across both branches:

```egg
(= outputs (If pred inputs then_ else_))
(= (Get then_ i) (Get (Arg arg_ty then_ctx) j))
(= (Get else_ i) (Get (Arg arg_ty else_ctx) j))
(HasType (Get then_ i) (Base (StateT)))
```

They subscribe to the original table ports; new rows, union/rebuild and missing
ports invalidate the corresponding instances. A single observed `i -> j` is
never assumed to be the complete map. One original RHS uses global `DUMMYCTX`.
The boundary compiler now admits its native zero-key table and follows that
table's committed values, rather than freezing an e-class ID. Computed LHS
primitive programs remain unsupported by this particular provider.

The JSON files select exact original rule names and anchors. Native compilation
validates the actual complete rules; they do not contain handwritten bindings,
precomputed answers, training instances or new `.egg` semantics.

## Timings

Each variant ran once, without counters, using the same newly built binary,
worker, frozen input, stable selection policy, joint proofs and CPU commit
protocol. Setup/Bake, graph serialization and Tiger extraction are outside
the inference column. Unchanged CPU controls were reused, not retimed.

| Variant | Setup | Complete inference | Change from baseline |
|---|---:|---:|---:|
| Baseline | 2.749 s | 13.944485 s | — |
| Tuple programs | 2.636 s | 12.745793 s | 8.60% lower |
| If programs | 2.987 s | 13.975751 s | 0.22% higher |
| Combined | 2.675 s | 12.720856 s | 8.78% lower |

The extra If group adds only 0.20% observed reduction beyond tuple programs.
There is no meaningful isolated If timing win in this measurement.

**Important conflicting evidence:** the preliminary pair, before rebuilding to
support the global port, ran baseline/tuple at **14.795926 / 14.712161 s**, only
**0.57% lower**. That pair used identical binaries internally and already had the
same tuple wiring. It remains in `results.json`; it is not replaced by the
larger later result. The final measured 8.6–8.8% is not established as a stable
gain, and we do not claim a parameterized-closure theorem from these timings.
The final run order was If, baseline, combined, tuple. No averages or best-of
selection were used. Keep the experiment optional pending stronger evidence.

## Search and maintenance cost

Separate counter runs show total GPU live candidate-row visits falling from
**10,405,823 to 7,193,043**. Only the seven selected queries move off GPU; the
305 other measured queries have identical fixed-rule-weight depth curves.
The removed queries have no remaining GPU seed population, so their new
conditional depth probabilities are undefined, not a 100% refutation rate.

| Selected query | GPU candidate rows before | After | Original complete bindings |
|---|---:|---:|---:|
| Get -> Add-Gets | single-atom path; not in depth counter | 0 | 22,672 |
| Add-Gets / Concat | 54,263 | 0 | 10,654 |
| Add-Gets / Single | 31,028 | 0 | 14,194 |
| Add-Gets / Not-Just-Concat | 15,252 | 0 | 1,981 |
| Add-All-Gets / length | 14,038 | 0 | 5,868 |
| If / non-weakly-linear | 1,664,447 | 0 | 0 |
| If / state-edge-passthrough | 1,433,752 | 0 | 4 |

The new CPU programs pay **129,746 root evaluations**, **98,911 index reads**
and **58,587 non-root candidate-row visits**. They reuse 973 port reads and
retain **62,131 cached port paths**. These are different units and must not be
added/subtracted to claim a net operation count. No RSS compression is claimed.
The zero-output If query was not an empty table: its correlated structural query
was actually evaluated. This is complete-query reuse, not RipSat absence-proof
credit. Original emitted binding counts are identical for every rule.

GPU dispatches decrease 2,761 -> 2,704; downloaded bytes decrease
17,424,828 -> 16,168,340. Native row events (2,425,040), union events (239,476),
6,890 ruleset runs and existing saturation-proof use remain identical. Logical
bindings, union events and proposed effects are not interchangeable with new
committed facts.

## Correctness and limits

- All timed variants pass the same-submit CPU comparison of original rows,
  values, equalities and visibility: **699,068 rows / 524,631 mapped e-classes**.
- The combined native-frontier audit checks **41,361 Cell-supplied queries**,
  including the added queries, with no mismatch. It does not audit the remaining
  GPU queries or replace the independent final-graph check.
- If and combined runs produce exactly the same selected DAGs for **all 29
  functions** as the same-submit CPU, using production Tiger. This does not
  measure execution speed of the generated raytracer.
- The pre-existing **fused-native versus same-submit** graph/Tiger disagreement
  persists. The three-lane extraction command deliberately retains exit code 1;
  `cpu-gpu-quality.json` passes while `quality.json` fails. No comparison is
  weakened or relabeled. This is not a pristine parity claim.
- 20 runtime library tests and 43 benchmark protocol tests pass, including new
  global-port union/late-insert/delete/reinsert and primitive-rejection tests.
- The first If attempt failed before inference with `global evidence is not
  supported`; its log is preserved in `out/parametric-if.log`.

We did not turn the full tuple recursion into one saturated transfer, discover
an affine branch mapping, or eliminate original effect execution. The present
evidence supports avoiding repeated query setup/search for selected families;
it does not yet establish the stronger “one whole closure instance/update” idea.

## Reproduction

Use the existing bench skill from this checkout; no new timing harness:

```sh
BENCH_SCRIPT=skills/cell-assisted-egglog-bench/scripts/bench.py
CONTROL_RUN=/Users/mineralsteins/.codex/worktrees/cell-assisted-egglog/egg_layout/out/pass3-compact-layout
METAL_WORKER=out/frozen-worker/matrix
DEVELOPER_DIR=/Library/Developer/CommandLineTools /opt/homebrew/bin/python3 "$BENCH_SCRIPT" run \
  --all-rules --joint-sat --no-cell-metrics \
  --worker "$METAL_WORKER" --reuse-controls "$CONTROL_RUN" \
  --boundary-programs experiments/pass3_parameterized_reuse/combined.json \
  --output out/parametric-combined-new
```

Choose each JSON for its variant and a new output directory. Add `--no-build`
only for an already verified binary. Use `--no-extract` to defer Tiger, not to
relax the graph check. Expected three-lane extraction failure is described above.

```sh
/opt/homebrew/bin/python3 "$BENCH_SCRIPT" boundary-credit --extension \
  --before out/parametric-baseline-v2 --after out/parametric-combined-v2 \
  --profile-before out/parametric-baseline-profile \
  --profile-after out/parametric-combined-profile --output out/new-comparison.json

/opt/homebrew/bin/python3 "$BENCH_SCRIPT" audit --all-rules --joint-sat --cell-only \
  --worker "$METAL_WORKER" --boundary-programs experiments/pass3_parameterized_reuse/combined.json \
  --output out/parametric-audit-new
```

Diagnostics use `CELL_SEARCH_COUNTERS=1` with a separate `run --no-extract`;
their times do not count as performance evidence. `comparison.json` is the
skill-generated work/depth comparison. `results.json` records every timed lane,
the preliminary smaller improvement, input/binary fingerprints, exact commands,
audit and failure status. Full graphs and runtime streams stay under ignored
`out/`. The main worktree is not modified by this experiment.

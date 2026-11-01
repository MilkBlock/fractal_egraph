# Observed multi-rule recipes: correct, performance gate failed

This experiment extends the **standalone CPU Cell engine**, not pristine egglog
or Metal. It remains opt-in (`CELL_CPU_COMPOSITIONS=1`) on
`codex/multi-rule-recipes`. It is not merged into the main workspace.

## Compilation and execution

The existing source → structural keys → source markers → typed Recipe IR
pipeline now has a backend-independent `compositions` pass before the CPU target.
It emits `composition-program.json`, records it in the manifest, and embeds the
same checked plan in `program.json`. `--composition-evidence` accepts observed
producer/consumer pairs tied to the exact typed recipe checksum. Benchmark
observations also record the originating program/report hashes. Observations
select edges; they are not a semantic proof or held-out generalization evidence.
The saved observations are rule-pair handoffs. Fact-position/slot projections are
derived and checked against the typed rules; this version does not yet export
full instance-level, multi-parent provenance from the original Analyze history.

The analyzer builds finite shared graphs rather than enumerating all paths:

- Source and target rule IDs, complete effects, original schedule, numeric guard
  positions, marker-entry field maps and exact correlated binding projections.
- Immutable predecessor facts and exact current keyed ports. An inverse or
  disconnected read which cannot be resolved functionally retains ordinary
  matching. Mutable/removable source rows cannot be permanent facts.
- Equalities from explicit union and constructor `set`. The native frontend
  commonly lowers `union(Single(x), s)` to a constructor result assignment;
  omitting that form would miss real equality evidence.
- Already committed constructor results which a successor can reuse. Prior
  producer unions normalize their keys. Consumer future unions are not moved
  ahead of intervening function updates or merge callbacks.

Rust independently checks every instruction against the typed original rules:
all atoms and effects remain represented, every keyed read has its inputs, every
reused result has the exact constructor key, and marker/control mappings match.
It does not merely accept a checksum. The entry marker identifies the shared
program through the compiler mapping and the Cell's execution cursor.

At an eligible original Run, a compiled entry fills the complete binding from
the predecessor context and current ports, then executes the original RHS in
order. Other delta pivots remain active for late inputs and alias alternatives;
the existing complete-binding map deduplicates overlaps. The cursor records the
program, entry instance and actual completed step count. It survives storage
takeover and checkpoint/resume. The original scheduler owns Run/Repeat/Saturate
visibility; no future Run is executed early.

A predecessor must have completed its RHS. The old executor can short-circuit
after a visibility action; facts before that action do not certify later effects.
There is a regression test for exactly this case.

This first version **does not replace all per-rule storage frames with one
saturated Cell body**, or certify an arbitrary complete closure. Known facts are
projected from the immediate producer's typed query/effects; it does not yet
collect every useful fact from the entire ancestor path. The graph and progress
are shared, and local results are continued, but that limitation matters for
search coverage. No additional global union/sat-skip permission is claimed.

## Validation

The SubTuple → Single → Get test executes three original rules from a marker
entry, reuses a committed Single result, retains constructor-result equality
proofs and reaches depth three. Both modes have identical per-Run graphs and
complete bindings, including a checkpoint after the producer and a fresh-process
resume. Other cases cover finite scheduling, late function ports, numerical
guards, union-created matches, removable facts and forged effect certificates.
All existing 52 regression programs are additionally recompiled with their
observed handoffs and checked through the new mode.

**40 Python tests and the Rust checksum regression pass.** The new diagnostics
also exposed an existing JSON float round-trip hazard in checkpoint checksums.
Enabling serde_json's exact float round-trip mode fixes it; the captured failing
values have a deterministic regression. The final binary was rebuilt and the
real-workload verification/timing repeated after this change.

The final sum-bits experiment passes all **1,623 Run boundaries**, complete graph
states and complete binding sequences against the same binary with the new
feature off. Source markers, reference-field copies, Cell takeover, selection
semantics and source schedule are identical in both lanes. Final logical records
are 10,339; live Cell bodies are 8,430; logical bindings are 45,911.

## Final single-run sum-bits Pass 3 measurements

| Metric | Off | On | Change |
|---|---:|---:|---:|
| Inference, including maintenance | 0.524768 s | 0.552117 s | **+5.2%** |
| Search phase | 0.264074 s | 0.282229 s | +6.9% |
| Candidate row visits | 686,858 | 686,858 | 0% |
| Complete-key lookups, all phases | 213,772 | 210,851 | −1.4% |
| Duplicate constructor requests reaching global lookup | 30,002 | 27,095 | −9.7% |
| Union requests | 4,627 | 4,627 | 0% |
| Function merge calls | 3,473 | 3,473 | 0% |
| Repair dependency visits | 38,283 | 38,283 | 0% |
| Repair collisions | 3,294 | 3,294 | 0% |
| Repair + takeover + retargeting time | 0.097916 s | 0.102411 s | +4.6% |
| Peak RSS | 231.44 MiB | 230.77 MiB | approximately unchanged |

Key lookup counts include constructor/RHS/repair uses; they are not pure LHS
search counts. Candidate visits, key lookups, union requests and merge calls are
separate units, not one funnel and not summed as “conflicts.” Repair timings
retain their existing combined bucket. No extraction or pristine comparison is
claimed here.

The library has 5 shared programs, 91 entry edges and 296,617 serialized bytes.
The composition pass itself took 0.134 s. The first device re-emission, including
receipt/artifact handling and validation, took 12.446 s; Bake and marker selection
were reused, not rerun. The final engine-only benchmark reused that artifact.

Runtime details:

- 2,330 original-rule continuation steps; maximum depth **5**.
- 76 continuation steps belong to entries supported by source markers.
- **12** known-fact atom supplies; 34 current port reads; 161 guard checks.
- 2,907 committed-constructor reuses; 429 reuse attempts fell back.
- 601 live cursors; the final Cell-body count did not decrease.
- Routing/evaluating the compiled match path took 19.87 ms; entry/cursor updates
  took 1.00 ms. These overlap the reported search/RHS phases and include useful
  work; they are not all pure added overhead and must not be added to total time.

Only 14 of the 2,921 avoided key lookups are beyond the 2,907 constructor reuses.
That agrees with the tiny known-atom count: this implementation mainly removes
duplicate construction lookups, not deep Join work. The full 27.35 ms regression
has not been separately attributed to allocation/cache/individual reads.

The source-frontier precheck still runs per ordinary delta seed. Source marker
coverage is weak for these recipes, and most complex inverse/multi-source joins
remain residual. A larger program graph or a five-step cursor alone does not
prove a larger reusable closure. Future work should enlarge the *certified fact
interface and binding coverage*, not claim success from graph/marker counts.

## Admission decision and reproduction

The benchmark implements the agreed gate: at least 10% lower total inference,
at least 20% reduction in a search/global-maintenance work count, and no more
than 10% RSS growth. Admission also requires two distinct audited guard workloads
with at most 5% time regression, using the same binary. Hit rate is not a gate.

**The primary performance gate failed. `merge_allowed=false`.** The two additional
small Pass-3 guard benchmarks were not run because the primary case already
rejects admission. No raytrace benchmark was run. This does not establish the
performance of those other workloads or the absence of a common-engine cost
relative to the previous commit.

```sh
# Compile from actual observed handoffs, verify, then measure once per lane.
/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.compositions_bench \
  --pipeline out/reference-pass3-owned/compile \
  --observed-run out/reference-regression-07/copy-takeover \
  --output out/new-compositions

# After an engine-only change, reuse the verified compiler artifact.
/opt/homebrew/bin/python3 -B -m experiments.cell_runtime.compositions_bench \
  --compiled out/compositions-sum-bits-01/compile --output out/new-engine-comparison
```

`--timing-only` is an exploratory final-state check, not full correctness.
`--audit-existing` performs the full check against the identical saved binary and
program; `--reuse-audit` reuses a checked fingerprint. `--guard-results` evaluates
two guard reports and only reports eligibility; the tool never merges Git state.
All five primary metrics and the explanatory counters are emitted automatically.

Raw final results are in `out/compositions-sum-bits-final`; the compact tracked
record is `composition-results/sum-bits.json`. The earlier binary measured
0.5270 → 0.5833 s (+10.7%); it remains under `out/compositions-sum-bits-01` and was
superseded after the checkpoint serialization fix, not selected for a faster time.

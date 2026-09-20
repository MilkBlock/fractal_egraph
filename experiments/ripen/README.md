# Ripen: native, isolated closed-rule-comb baseline

Ripen is the term for driving an explicit local entry toward its smooth closure.
The first implementation uses a fresh native egglog EGraph for one input file.
It does not create per-instance global tables or a second matcher. Source actions
provide the entry; all declared rules participate, grouped into deterministic full
ruleset sweeps. Native `RunReport.updated` decides whether a complete sweep changed
anything, including union/rebuild effects. Budget exhaustion is not closure.

```sh
cargo run --release -- ripen experiments/ripen/union-entry.egg out/ripen-demo --max-rounds 8
cargo run --release -- ripen experiments/ripen/growing-entry.egg out/ripen-growing --max-rounds 4
cargo run --release -- analyze --replay-history out/ripen-demo/history.json --output out/ripen-replay
```

Use fresh output directories. A round is a full sweep over all declared rulesets;
it does not bound the cost of one individual join or force an interrupt mid-round.
The default budget is 32 sweeps. Unlike `analyze`, `ripen` takes an explicit entry
file without a `run` schedule. The first version accepts one self-contained
datatype, rules/rewrites, ruleset declarations, constructor/let/union actions and
postconditions (`check`). It rejects include, custom functions, set/delete/subsume,
other schedules and unsupported commands rather than silently omitting them.
Postconditions execute only after closure, otherwise they are marked deferred.

## Feedback to Tier1

Each sweep drains native trace events through the existing importer. Actual
eligible applies enter `LayerStore` and its normal Use/template builder. A
`RipenFeedback` attached to the store and round boundary records:

- `Growing`, `Closed`, or `Suspended`;
- round/budget and full ruleset scope;
- whether the native engine reported updates;
- how many matches the existing trace importer excluded.

**Closed describes the entire isolated cell for this fixed entry and ruleset.**
It is not inherited by every sub-Use and does not establish a parametric rule for
other entries. Engine fixed-point observation is independent of trace coverage:
excluded matches remain visible; the Tier1 history is not asserted to be a complete
reconstruction of the local engine's effects. New external facts/union require a
new ripen run; no automatic runtime substitution is performed.

Outputs:

- `ripen.json`: whole-cell result, exact source/rules and Tier1 feedback;
- `entry.egg`: original explicit entry;
- `ripened.egg`: normalized source plus the performed schedule; runnable in egglog;
- `history.json`: resolved history including ripen round metadata;
- `rounds/`: per-sweep JSON and the existing five DOT views.

Load `out/ripen-demo` in the existing tools debugger and choose Coarse / Smooth
layers. The `ripen-cell` node shows the whole-cell state and scope. CLI replay
preserves the round metadata and generates identical JSON/DOT snapshots.

## Evidence and boundaries

The union fixture runs the consumer before the producer. The first sweep merges
A/B; the next enables Pair(x,x), creates Done and unions it with the root; the
third observes no update. Its native `(check (= root (Done)))` passes. Limiting the
same fixture to one sweep reports Suspended. The growing numeric fixture remains
Suspended after four sweeps. Tests also execute the generated `.egg`, replay the
saved history and reject subsuming rewrites.

This is actual patched egglog execution, not a simulated match history. It is a
baseline, not a speedup result. Automatic extraction of arbitrary Use entrances,
batched cell tables, proven Fractal-summary execution and cross-instance Bake
reuse are not implemented here (`fractal_summaries_used` is explicitly zero).

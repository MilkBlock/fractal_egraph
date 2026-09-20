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

## Automatically extract a symbolic Use entry

```sh
cargo run --release -- ripen-use out/use-reuse-final/math/history.json 24 out/ripen-use-final --max-rounds 8
```

The Use ID is resolved by the **current default Tier1 builder** replaying this
history; it is not a globally stable ID. Non-default admission/cut/rotation flags
are rejected. `origin.json` records source event/member IDs for inspection.

The extractor imports all recorded source rules, checks that they cover the
source rule declarations, and recovers the selected Use's LHS skeleton from its
recorded bindings and producer ports. Unknown e-class boundary values become
fresh `RipenInput(i64)` constructor terms in the original datatype, with captured
aliases preserved. These terms explicitly stand for opaque parameters; they are
not guesses of the original concrete expressions. Primitive boundary values whose
literals were not recorded are rejected. The history datatype name is now read
using egglog's AST parser, fixing the former whitespace-split extraction of the
word `datatype` instead of the actual sort name.

Before ripen, a separate native EGraph checks each original member's ground LHS,
executes only that member's ground actions, and checks recorded output aliases.
Failed equalities are not repaired by injecting unions. Internal rows are not
seeded, and an external row whose nested AST would construct an internal read is
rejected. External producers after the first member, unversioned later inputs,
and unsupported actions/rule coverage also fail explicitly. This deliberately
conservative subset can reject otherwise valid interfaces.

Generated files:

- `entry.egg`: initial symbolic interface + all source rules + postconditions;
- `validate-use.egg`: executable, staged checks/actions for the original Use;
- `origin.json`: boundary parameters, source members and validation metadata;
- `run/`: normal ripen result, Tier1 history and per-round DOT;
- `result.json`: origin and ripen result, including per-constructor table sizes.

`RipenOrigin` is attached to the resulting Tier1 feedback and persisted in history
rounds. It links back to the source history/Use/template and explicitly labels the
symbolic boundary. No original history or previously generated analysis is changed.

**Closed applies to the extracted symbolic interface.** Substituting concrete terms
for the opaque parameters can enable additional matches inside those terms, so
this is not proof that the full original tier0 neighborhood or all parameter
instances are closed. Automatic concrete boundary reconstruction, staged coarse
injection, importing this as an executable global macro, and Fractal-summary
acceleration remain outside this implementation.

### Math6 check

U24 (T12, source records 109/134) passes both staged original-rule checks. Its
initial interface has 2 Add table rows; replaying its two members produces 5;
ripen with all 24 source rules reaches 12 rows and reports Closed at sweep 4.
There are 18 imported local apply records and no excluded matches. Row counts are
net canonical table sizes, not counts of all mutations or speedup measurements.
U146 is rejected because seeding its outer external read would create an internal
read early; it requires staged injection. Results refer to the existing Math6
history and the current builder, not arbitrary history IDs.

Native tests independently capture an A→B→C→D workload, extract an A→B→C Use,
verify B/C were absent at entry, and show ripen additionally derives D. Tests also
execute `validate-use.egg` and reject unavailable primitive boundary data.

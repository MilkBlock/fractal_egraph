# SaturatedRuleComposition sharing

A successful ripen now exports `saturated-rule-composition.json` when its semantics are supported.
The export reads every datatype constructor table directly from the native engine,
canonicalizes values through union-find, and keeps all constructor rows, ordered
fields, exact scalar literals, cycles, sharing and named global ports. It does not
use the filtered match history or table sizes as a substitute for these facts.
`local_ids` are canonical local IDs at export time, retained only for provenance;
they are not semantic labels and are ignored by equivalence comparison.

The initial sharing contract supports one equality datatype, i64/String/bool
constructor fields, equality-sort named globals, and positive constructor/equality
rules. Other primitive guards/actions, containers and unsupported sorts produce
an explicit `unavailable` export status; ripen itself may still report Closed.
Constructor cost/unextractable flags are read from the actual declaration AST.
This is a conservative contract, not a proof for every egglog primitive.

## Algorithm

1. Compare datatype/constructor schemas, rule theory, symbolic-boundary policy,
   exact literal inventory and fixed named ports. Rule names, declaration order
   and ruleset grouping are ignored; source variable names and atom ordering remain
   conservative scope distinctions. No general theory-equivalence solver is used.
2. Refine the joint typed incidence graphs until their color partitions stabilize.
   Facts are vertices connected to argument/result values by distinct field roles.
   This is an invariant/candidate filter, not an equivalence certificate.
3. Search for a type/color/port-preserving value bijection under a state budget.
   Verify the *entire* mapped fact set, including additional rows. Equal row counts,
   equal extracted endpoints or matching colors alone never establish equality.
4. Intern a representative SaturatedRuleComposition only after exact confirmation. Preserve each
   Trigger's entry, binding origin and value-to-representative mapping separately.

Results distinguish `equivalent`, `different`, `incompatible_scope`, and
`unknown_budget`. The comparator caps graphs at 512 values / 4096 constructor rows;
these capacity stops also return UnknownBudget (zero explored states). Oversized
states can still be exported. The search budget counts attempted value assignments.
Unknown candidates are stored separately, not declared semantically different;
`unresolved_comparisons` exposes this. State IDs are local to one catalog and depend
on insertion order. This is representative interning, not a globally canonical
labeling or an optimal/minimal catalog under budget limits.

For N incidence vertices and M field edges, refinement performs at most N rounds;
with bounded labels/arity a conservative bound is O(N(N+M) log(N+M)). Exact search
can be factorial without a budget. With B attempted assignments, the current
implementation additionally performs bounded candidate scans and full-row checks,
roughly O(B(V^2 + F*d*log(F+1))) for V values, F rows and maximum arity d. Candidate
storage can be O(V^2). These are not claimed to be unconditional near-linear bounds.

## CLI

```sh
cargo run --release -- ripen experiments/saturated-rule-composition/from-a.egg out/closed-a
cargo run --release -- ripen experiments/saturated-rule-composition/from-b.egg out/closed-b
cargo run --release -- saturated-rule-composition-compare out/closed-a/saturated-rule-composition.json out/closed-b/saturated-rule-composition.json --budget 10000
cargo run --release -- saturated-rule-composition-catalog out/closed-ab out/closed-a out/closed-b
```

A and B start from different facts and declare the rules in different orders with
different names. Both native runs close to A=B and retain the same `root` port.
They produce one state and two distinct triggers.

For the existing Math6 history:

```sh
cargo run --release -- ripen-use out/use-reuse-final/math/history.json 24 out/saturated-rule-composition-unify/u24 --max-rounds 8
cargo run --release -- ripen-use out/use-reuse-final/math/history.json 84 out/saturated-rule-composition-unify/u84 --max-rounds 8
cargo run --release -- saturated-rule-composition-catalog out/saturated-rule-composition-unify/catalog out/saturated-rule-composition-unify/u24/run out/saturated-rule-composition-unify/u84/run
```

Use fresh output paths. Current default-history U24/T12 has two members; U84/T18
has three members. Both produce one equivalent symbolic saturated rule composition: 7 Math
classes + 3 i64 values, with 12 Add rows + 3 opaque RipenInput rows. The exact
comparison visits 10 assignments. The catalog preserves both triggers and their
source binding maps, and stores the common state once.

The files are `catalog.json`, `catalog.dot`, and `states/state-NNNN.json`. The
existing tools debugger includes a **SaturatedRuleComposition 共享目录** panel. Load
`out/saturated-rule-composition-unify/catalog`, or open
`http://127.0.0.1:8080/?saturated_rule_composition_catalog=out/saturated-rule-composition-unify/catalog`.
Click each trigger to inspect its origin/value mapping, or the shared state to
inspect its complete facts. It uses the existing Graphviz renderer.

This unifies mature **state definitions**, not trigger conditions or live e-classes
from separate instances. It does not rewrite tier0 rules, prove arbitrary boundary
substitution preserves closedness, or establish equivalence from different rule
semantics. Symbolic Use boundaries remain symbolic. The closure-under-extension
argument assumes the same positive monotone semantics and appropriate interface
mapping; exported files are trusted recorded observations, not independently
rechecked fixed-point proofs across engine revisions.

## Checks

```sh
cargo test --release --test saturated_rule_composition --test ripen
```

Tests cover renamed cyclic/shared graphs, differing port targets, alias changes,
exact scalar literals, different facts with equal counts, theory mismatch and
budget exhaustion. A six-cycle versus two three-cycles exercises exact rejection
after indistinguishable refinement. Native tests verify different triggers merge,
scalar/global-port differences do not, and unsupported primitive semantics cannot
silently enter the shared-state catalog.

# Generic analyzer Cell recipes

Analyze now automatically publishes `cell-recipes.json` and a compact
`cell_egraph.generic_recipes` summary. The representation is independent of
specific rulesets, constructors, helper names and arithmetic patterns. It is
separate from the specialized v1-v3 Metal recipes in `eggfuse/compiler/recipes/executable.py`.

## What is exported

Every successfully prepared Cell unit gets:

- Shared native declarations/rules and an interned **syntax** DAG. Rule variables
  stay lexical to each invocation; boundary input slots are local to each Cell.
  This does not memoize a mutable function read across a `set` or union.
- Ordered preparation and replay-validation instructions. LHS guards,
  pre/post-update checks, external injections, relative bindings and the original
  replay-stage boundaries remain. Conditions/actions are structured nodes, not
  opaque strings. Native declaration syntax is retained for sorts/signatures;
  merge expressions also have checked structured references.
- An explicit isolated-ripen closure program. It preserves that local ruleset
  order, not the unrelated tier0 schedule. Running this closure program still
  uses the native matcher when no query-free lowering is available.
- For a completed finite state: `literal`, `value_slot`, and `fact` instructions
  describing every row, exact scalar value, equality-sharing/cycle, container
  membership and subsumed flag. Input-to-initial/final-slot mappings are kept.
  This is an endpoint batch, not an ordered log of intermediate merge requests.

Environments, expression syntax and instruction syntax are shared. A library is
rewritten only when its contents change, and atomically replaces the previous
complete file. It does not turn off Ripen's existing 50 ms / 200K observation
limits or introduce an additional discovery budget.

Every candidate also remains in the manifest. Preparation failure is reported
as `unavailable`; an unfinished Cell retains a program but has no fabricated
finite endpoint. Existing unit-to-eclass mappings remain in `cell-egraph.json`,
so several composition paths for one Cell are not silently discarded.

## Authorization and execution

Three statements must remain separate:

1. A **prepared program** can replay its original checks and actions.
2. An **observed endpoint** can be reconstructed exactly for inspection and
   checked against the native local execution.
3. A **certified reusable closure** has the existing stronger sharing contract.

The new exporter does not promote (1) or (2) to (3). In particular, custom
merges, primitive guards and containers still encounter limitations in the old
sharing-certification layer. `live_substitution_authorized` is false. The generic
library is not automatically accepted by the specialized Metal sat-skip path.
A runtime lowering must supply the input/scope/version guards and valid stage
publication; an endpoint must not be published early into a larger context.

The standalone verifier loads the library without the training source. It
executes preparation/validation and bounded native closure, reconstructs the
endpoint via the native terminal snapshot importer with **zero rule executions
and zero rebuilds in the materialization step**, and compares complete typed
facts, values, equality, visibility and named boundary ports. A regression keeps
the same fact graph but reconnects a named root to the wrong value, and must fail.
Native closure used as the oracle still
performs matching. This is not an end-to-end GPU speedup measurement.

## Bugs found while generalizing

- The state exporter previously assumed a `relation` returned `Unit`. This
  kernel may reify it as a separate equality sort. Export now reads the installed
  native schema, fixing reconstruction of relation facts.
- An equality-sort function update was rejected when its merged result had no
  prior symbolic name. A declared constructor merge such as `Join(old,new)` now
  supplies that expression from the recorded old/proposed values. Before/after
  checks still run in native egglog; operands involving unstable mutable reads
  are not guessed. A regression falsifies the committed receipt and must fail.

## Recorded checks

These use actual Analyze and native local execution, not invented Cell records:

| Input | Prepared recipe units | Verified finite endpoints |
|---|---:|---:|
| Real Pass-3 `terms` rules, small facts | 72 | 72 |
| Real Pass-3 `is-resolved` rules, small facts | 19 | 19 |
| Mixed guards, min updates, relation and Set | 3 | 3 |
| Pure constructor/union, including cycles | 3 | 3 |
| Unbounded counter growth | 1 | 0, correctly unfinished |

The terms input has **89 candidates**. The merge fix raises successful
preparation **33 -> 72**; the remaining **17** fail existing action/injection
replay checks and are retained with reasons. The verified 72 are observed
endpoints, not newly certified sharing proofs. The pure constructor case has
three certified endpoints. These are representative rule/input fixtures, not a
new whole-Raytrace coverage figure.

## Why the earlier runtime gain was small

The paired full runtime measured about 6.25 s of joint terms GPU work inside a
42.92 s inference. Saving about 0.86 s there cannot by itself create a large
whole-program speedup. The 81,104 boundary tuples and final graph size were
unchanged; other rulesets, repeated stage/context maintenance and publication
remained. The historical 40 s versus later 42 s common slowdown was not isolated
and is not attributed to one cause here. No new GPU timing is claimed for this
analyzer-only change.

## Reproduce

```sh
CARGO_BUILD_JOBS=8 CARGO_PROFILE_DEV_DEBUG=0 cargo build --offline \
  --bin egg_layout --example cell_recipe
python3 experiments/generic_cell_recipes/run.py --output out/generic-recipes-new \
  --pass3-source /path/to/pass-3-flat.egg
# Inspect an existing Analyze directory, without its source:
target/debug/examples/cell_recipe ANALYSIS/cell-recipes.json verification.json all
# Reuse already verified records:
python3 experiments/generic_cell_recipes/run.py --output EXISTING --read-only
```

Core code: `src/cell_recipe.rs` (library/compiler), `src/cell_recipe/codec.rs`
(native AST decoding/execution), `src/cell_recipe/endpoint.rs` (row batches and
exact comparison), `src/native_cell_egraph.rs` (automatic registration), and the
single publication hook in `native_saturated_rule_composition_pipeline.rs`.

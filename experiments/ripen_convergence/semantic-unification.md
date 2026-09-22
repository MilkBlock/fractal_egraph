# Typed semantic composition of constructor rewrites

Implementation: `src/semantic_compose.rs`. This follows Ant's boundary-unification
idea, independently implemented for the positive egglog constructor fragment.
The reference is pinned in `ant-reference.md`; no new upstream source is copied.

## API and semantics

`compose(A, B, position)` first standardizes B's variables apart, then unifies
`A.rhs[position]` with `B.lhs`. Unification checks sorts, constructor names/arity,
literal identity, repeated-variable constraints and the finite-term occurs check.
A failed unification leaves the caller's substitution unchanged.

The resulting trigger is the substituted A.lhs; its endpoint replaces the selected
subterm of A.rhs with substituted B.rhs. Crucially its effect ledger retains ALL
union pairs from A and B. It is not merely a shortcut from initial to final term.
For example, swap composed with swap has identical endpoint and trigger but still
adds the swapped enode. The substitutions, right-variable offset and selected
position are serialized, making this more than an opaque Concat node.

Equality and declared-relation guards are substituted and retained on the new LHS.
They are conservatively required at entry. A guard that only becomes available
after the first union is not automatically discharged: this can miss a valid
composition but does not allow an invalid application. This is not a claim of the
most general guarded e-graph composition. Pure finite-term boundary unification is
most-general within its typed syntactic fragment.

The exporter emits an actual egglog `(rule ...)`, including all intermediate union
effects. Tests run exported rules in native egglog with fresh bindings, compare
retained Add counts and expected terms, and check absence/presence of a relation
guard. Two swaps retain two Add enodes, agreeing with sequential execution rather
than falsely collapsing to an effect-free identity. IBP self-composition at RHS
child 1 is also executed natively: one combined application agrees with two
original rounds on each constructor count, including 2 Diff and 5 Integral rows.

## Source and command

The importer uses egglog's own parser, not a new expression parser. It handles
constructor `rewrite` commands over declared datatypes, literals, bound equality
guards and declared-relation guards. It does not import arbitrary `rule`, subsuming
rewrites, fresh RHS variables, primitive arithmetic/actions or unrecognized guards.
Unsupported rules are absent from the returned rule book; the CLI reports them as
unavailable. The existing original rule remains executable by normal egglog.

```sh
cargo run --release --offline -- compose-rules \
  experiments/ripen_convergence/math-right.egg assoc comm out/compose-root
# Select child 0 of the first RHS, rather than its root:
cargo run --release --offline -- compose-rules \
  experiments/ripen_convergence/math-right.egg assoc comm out/compose-inner 0
```

Use fresh output directories. Outputs are `combined.egg` and `unification.json`.
The .egg is a rule fragment to be loaded with the original datatype declarations.
Dot-separated positions such as `0.1` specify nested RHS children. Invalid paths
and incompatible boundaries fail explicitly.

## Native library integration

Version 2 of `continuations.json` adds `semantic_compositions`. On donor completion,
the learner traverses EXISTING Concat nodes (no all-pairs enumeration) and derives
root-position compositions where both children have supported summaries. Automatic
candidates are capped at 16 packets each. This cap bounds candidate depth, not total
term size; large source patterns and substitution duplication still cost memory.

Each candidate includes its source node, typed trigger/endpoint, accumulated
effects/guards, substitution and executable .egg text. Each evidence object records
its candidate IDs. The Add-AC fixture produced 16 root-position candidates.

`observed_position_verified` remains false: packet history currently records effects,
not a complete syntactic LHS position witness. A root composition can be a sound
new rule without describing the particular historical pair (which may have applied
at an inner position). Candidates are therefore NOT silently substituted for the
recorded packet sequence or installed as new native runtime rules. Continuation
reuse still uses exact whole-state verification.

This is finite typed term unification, not e-unification modulo arbitrary unions.
The occurs check rejects cyclic substitutions even though an e-graph may represent
cycles. Relation guards are retained, not solved; arbitrary coarse-context joins
and automatic position inference remain outside this implementation. Native engine
cloning has not been replaced by parameterized macro execution.

```sh
cargo test --release --offline --test semantic_compose --test packet_library \
  --test ripen_convergence --test saturated_rule_composition_pipeline
```

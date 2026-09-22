# Candidate-local difference explanation

The general rewrite unifier, `compose-rules` command, automatic Concat semantic
candidate generation and semantic-candidate library fields have been removed.
The project goal remains early detection of shared saturated contents, not growing
a general shortcut rule library. Packet storage, incremental fingerprints, exact
rendezvous checks and certified completed-continuation reuse remain intact.

`src/state_difference.rs` operates on exactly two supplied states. It aligns fixed
named ports, optional caller-supplied anchors, and unique typed scalar literals.
An indexed work queue propagates bindings through equal complete constructor/table
keys. It does not enumerate graph mappings, candidate pairs or rewrite combinations.
Ambiguous keys, unanchored cyclic components and conflicting aliases remain explicit.

```sh
cargo run --release --offline -- ripen-diff \
  LEFT/saturated-rule-composition.json \
  RIGHT/saturated-rule-composition.json \
  out/state-difference.json
# Optional final argument: a JSON array of [left_value_id, right_value_id] anchors.
```

The report distinguishes:

- `same_contents`: the completed alignment preserves rows and visibility;
- `different_contents`: completed alignment with directional missing rows or
  visibility changes;
- `partial_alignment`: unresolved value correspondences or conflicting results;
- incompatible rule scope/ports or an explicit interface alias conflict.

`left_only` lists left facts absent on the right, expressed in right-side variable
IDs. `right_only` lists right facts absent on the left. These are potential proof
goals: the right needs to derive `left_only`, and the left needs to derive
`right_only`. Visibility differences and equality conflicts are separate obligations,
not automatically discharged by adding rows. Unmapped rows are reported separately,
not incorrectly declared absent. Value equality is used for content alignment, not
as a causal provenance edge.

Example: with fixed ports a, b and root, states containing only Add(a,b)->root and
Add(b,a)->root produce one missing row in each direction. This diagnostic does not
infer commutativity or create a rewrite. A native derivation witness is still needed
to prove those obligations. Even `same_contents` does not by itself certify identical
pending injections, scheduler state or a shareable saturated continuation; the
existing contract checks still govern that decision.

This command does not expand the online candidate mechanism or authorize reuse.
There is currently no automatic difference-directed ripen scheduler. It is a narrow
inspection/verification building block for a pair already selected by the caller.

The work queue revisits rows only as argument mappings become available. For bounded
arity, indexing/propagation avoids graph-search backtracking; wide rows can still
incur repeated argument scans. The final ordered row comparison and literal indexing
have logarithmic map costs. No claim of constant-time state comparison is made.

```sh
cargo test --release --offline --test state_difference --test packet_library \
  --test ripen_convergence --test saturated_rule_composition_pipeline
```

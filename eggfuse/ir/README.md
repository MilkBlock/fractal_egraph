# Shared Recipe IR

`eggfuse-recipe/v1` is an executable typed program contract. One rule has a
correlated slot frame, full query atoms and an ordered effect program. RHS
assignments get distinct definition slots; literals, repeated variables,
primitive conditions and exact types remain explicit. Source labels are kept
for lossless native-core roundtrip and diagnostics.

The control tree retains Run, finite Repeat, nested Saturate and supported
assertion/probe fields. `original_program` and `isolated_cell` are different
scopes. An isolated endpoint does not gain global substitution authority by
being serialized or re-signed. Generic and specialized lowering reconstruct the
same reference core; each fast path still needs its typed regional proof.

The verifier checks call/operand types, references and definitions, then
re-derives the canonical representation and compares it with the supplied
contract. Engine loading also compares it with the actual native typed source.
A valid different program is not accepted as the original program.

Read/write/union footprints and event routes are derived information. Direct
function writes include declared merge-expression table accesses. Equality
invalidation lists are conservative hints, not a complete proof of all indirect
global rebuild effects. No optimization currently relies on these hints to skip
unknown work. Event routes are shared per table, retain same-row constraints and
residual atoms, require the original stage gate, and consume the actual
post-merge canonical visible row. A proposed `set` value is not its resulting row.

`eggfuse-recipe-library/v1` adapts native `analyzer-device-cells/v1` exports. It
retains actual initialized instances and their completion status, not stored
terminal answers. The source program is separate from isolated ripen programs;
its original control is not replaced by the isolated joint saturation schedule.

Current limits: no new arbitrary Cell partition proof, no generic installed
event subscribers, no elimination of all queries, no automatic authorization of
unfinished/nonterminating instances, and no repair of the existing complete
Pass-3 native/device difference. The initial IR favors inspectable exact semantics
and compatibility, not minimum serialized size or GPU-resident metadata size.

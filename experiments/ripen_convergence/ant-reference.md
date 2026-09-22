# Ant implementation reference and local adaptation

Source reviewed: MarisaKirisame/ant, local checkout
`/Users/mineralsteins/Repos/ant`, commit
`3f698ae0668870bef6238bbd6e86d957c0e08647`. The reviewed files were clean.
The public GitHub repository page was also inspected; the GitHub API refused a
fresh HEAD request due to rate limiting. This is a pinned implementation review,
not a claim that this local commit is the latest upstream revision.
Upstream is Apache-2.0. The Rust code here is independently implemented from the
data-structure ideas; no upstream source was copied or vendored.

- [Words.ml](https://github.com/MarisaKirisame/ant/blob/3f698ae0668870bef6238bbd6e86d957c0e08647/lib/Words.ml):
  `measure` carries length, degree, max_degree and monoid hash. `append` uses a
  finger tree; `slice_length` and `pop_n` split using those cached measures.
  `equal` in this revision compares hash values directly. We instead retain
  structural Eq for interning and exact local-state verification for reuse.
- [Hash.ml](https://github.com/MarisaKirisame/ant/blob/3f698ae0668870bef6238bbd6e86d957c0e08647/lib/Hash.ml):
  `MonoidHash` exposes unit/mul; `MCRC32C` calls native stubs. Our ordered hash uses
  wrapping polynomial composition and cached powers, not hardware CRC32C.
- [Value.ml](https://github.com/MarisaKirisame/ant/blob/3f698ae0668870bef6238bbd6e86d957c0e08647/lib/Value.ml):
  Word/Reference nodes support partially materialized values; the full length/hash
  summary becomes unavailable for unresolved references. We keep explicit binding
  vectors and do not assume unresolved inputs have a known content hash.
- [Memo.ml](https://github.com/MarisaKirisame/ant/blob/3f698ae0668870bef6238bbd6e86d957c0e08647/lib/Memo.ml):
  `inc` is a binary carry over adjacent slices, with `compose_slice` preserving
  chronological order. Each executed atomic OR shortcut step is added to the
  counter. A counter level therefore need not mean equal primitive-work counts;
  `step.sc` separately records work. Our lengths count recorded packets, not
  skipped native rounds or primitive operations.
- [Dependency.ml](https://github.com/MarisaKirisame/ant/blob/3f698ae0668870bef6238bbd6e86d957c0e08647/lib/Dependency.ml):
  `compose_step` unifies the first destination with the second source, checks the
  program-counter boundary and reconstructs source/destination patterns.
  Concat remains an immutable evidence node; the separate semantic-composition
  pass now unifies supported typed constructor rewrites and preserves guards and
  intermediate effects. Exact-state rendezvous remains the execution-reuse proof.

## Adaptation

`src/packet_library.rs` stores normalized effect templates, Apply nodes carrying
binding vectors, and Concat nodes referencing children. A two-piece sequence hash
is composed as `hash(XY) = hash(X) * power(Y) + hash(Y)` with wrapping u64 arithmetic;
length and power are cached. Hashes are library-local, not a cross-library canonical
identity. Tree shape can differ while the sequence hash is equal; hash equality
alone never authorizes structural interning or semantic equivalence.

The binary-carry builder creates O(n) nodes for n packets, before hash-consing.
Its generated trees have logarithmic height; suffix slicing shares interior nodes
and visits their boundary paths. Arbitrary client calls to `concat` can build an
unbalanced tree; this is a small persistent concatenation DAG, not a full finger
tree supporting arbitrary balanced edits. Degree/max_degree are not needed here:
packet boundaries are explicit, unlike Ant's flattened constructor-word stream.

Variables are numbered at first occurrence inside each packet, including repeated
occurrences and aliases. Each Apply retains a binding vector into its evidence
object's local variable space. Different evidence objects may reuse the same
structural nodes without identifying their variables. Rule names plus effect
shapes form templates; required LHS/guards remain in the source/evidence contract.
These templates are NOT executable rules and do not certify trigger equivalence.
Packets preserve native event order within an application; their sequence follows
application IDs within each drained batch. It is a provenance serialization, not
a proof that arbitrary independent applications commute.

Real Add-AC fixture: 18 committed-effect packets, 4 templates, 35 rope nodes
(including slice construction), one shared evidence object. A synthetic test with
4096 identical packets needs 13 nodes; this is a structural test, not a runtime
compression result. The original native engine clone is still used for execution
reuse, and full donor Tier1 evidence is still stored once. No total-memory or speed
improvement is claimed from these counts.

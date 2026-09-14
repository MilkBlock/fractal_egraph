# FractalRule Bake

Bake is an offline phase over small **tier0** samples. Frozen use does not call
the discovery pipeline on a new sample. Each command runs in one Rust process.

```sh
cargo run --release -- bake experiments/bake/manifest.json out/my-bake
cargo run --release -- bake-use out/my-bake/library.json \
  experiments/bake/heldout-binary.egg out/my-use --save-history

# Counter successor values, start=31, limit=200:
cargo run --release -- bake-eval out/my-bake/library.json \
  fractal_0000 31 200 out/counter-query.json

# Complete binary frontier at depth 20, starting at 11:
cargo run --release -- bake-eval out/my-bake/library.json \
  fractal_0001 11 --depth 20 out/frontier-query.json
```

Output paths must be new. Template IDs are local to a particular library; consult
its `bake.json` rather than assuming these example IDs in a different library.

## What gets baked

The manifest has `version: 1` and at least two uniquely named samples. Each
sample specifies `source` relative to the manifest and optional `rounds`.
Omitting rounds preserves the source schedule; an override only accepts a simple
run, and source checks remain active. The input adapter currently requires one
self-contained Math datatype, built-in primitives and constructor/union rules.
Custom functions/sorts and includes are rejected. Rule variable names must not
overlap top-level global names, avoiding implicit global assumptions in a reusable
contract. Subsuming rewrites are not supported by this capture path.
Tier2 array DSL scripts are not tier0 capture inputs.

For each sample, Bake runs the existing native capture, tier1, FractalComb and
recursive-template discovery. It merges linear and A→mB→A families by portable
source/binding signatures. Variable and rule names, source offsets, sample values
and event IDs are excluded from template identity. Operator identities, literals,
guards, relevant constructor types, input/output roles and relative routes are
retained. Recursive families also retain alias and committed-effect shape.
Linear repeats retain the prior policy: occurrence aliasing is not assumed fixed.

A library contains shared steps, template interfaces, local binding/requirement
DAGs when exportable, source snapshots, and sample-qualified witnesses. Raw traces
are not written. Event numbers from different samples never identify one event.
A template can have only one supporting sample; the support counts expose this,
rather than pretending every family was confirmed by every sample.

Outputs:

- `library.json`: fixed library, versioned and checked when loaded.
- `bake.json`: family list, sample support and available scalar-array summaries.
- `baked.egg`: native DSL data recipes for inspection/reduction. These are local
  contracts and arrays, **not** unconditional tier0 replacement rules. Symbolic
  example parameters must be instantiated with their law's conditions; do not
  bind shared placeholders by global unions across unrelated contexts.

## Frozen use

`bake-use` computes portable keys only for newly collected records and probes the
fixed library indexes. It does not construct tier1/tier2 graphs, enumerate new
candidate families, extend the library or silently invoke discovery on a miss.
At an observed entry it creates a pending instance of a known template. Branches
and returns attach as later records arrive. Partial and ambiguous instances are
not reported as complete. Matching runs at the existing capture boundaries:
simple run rounds, or completion for complex schedules.

`use.json` contains input bindings, actual member/return event IDs, unique event
coverage, uncovered reasons and per-boundary progress. Optional `history.json`
can be replayed through ordinary `analyze` for debugging or an explicit audit.
These are historical occurrence matches, not a claim that every old physical
row version is still live in the final egraph.

**Tier0 still executes its full original schedule in this mode.** The savings
are in reusing templates and avoiding metadata graph construction/discovery.
A missing match may mean a new family, a changed binding/effect shape, a warmup
boundary, or a limitation of the current single-parent family detector. It is
not automatically evidence of dissipation.

## Array summaries that can bypass tier0

A separate checked source-schema path currently recognizes two generic forms,
without testing rule names or memorizing sample answers:

1. `C(i,end,rest)` with `i < end` → `C(i+1,end,rest)`, distinct local fields and
   i64 index/limit. Remaining steps are `max(0,end-start)`. Every successful
   update is at most end and cannot overflow. Successor payloads form a ramp.
2. `A(n)` with `n > 0` → all `B(radix*n+r)` for residues `r=0..radix-1`, followed
   by the unconditional payload-copy return `B(n) → A(n)`. At depth d, the
   complete frontier is `start*radix^d+j`, `0<=j<radix^d`. The residue partition
   establishes this by induction. Queries require the entire frontier to fit
   i64, so all earlier primitive operations are defined too.

The rules must have the exact supported form: hidden equality requirements,
extra guards, changed residues or unrelated actions do not receive these proofs.
This is a small set of structural proof schemata, not a general induction prover.
Unrecognized families remain useful observed templates, without a tier0-free
query capability.

`bake-eval` rechecks the schema and domain, constructs the corresponding first-
class RampArray, and asks the existing native array/Reduce DSL for its sum. It
performs no tier0 application, element query or prefix expansion. It returns a
**value projection of an isolated recurrence**, not a replacement for an entire
fixed-round program, and does not materialize its intermediate facts/unions.
Machine-integer operations that cannot be folded remain symbolic; impossible
complete-frontier depths are rejected. Large sums can remain exact symbolic
expressions rather than overflowing an i64 accumulator.

## Validation and ablation

The four training files produce two shared families. Held-out files change initial
values, depth, variable/rule names and an unused constructor. The +2 counter is a
negative holdout: it must stay uncovered and must not change the library.

```sh
cargo test --test bake --test arrays
cargo build --release
python3 experiments/bake/benchmark.py --output out/bake-validation --repeats 3
```

The optional Python script is only an experiment harness, not the runtime
pipeline. It compares fixed matching with full metadata analysis of the exact
same saved capture and checks identical covered event sets. Full analysis also
builds graphs and richer diagnostics; its time ratio is not a tier0 speedup.
Small frontier queries are independently checked against real tier0 rows. The
million-element query checks that the array stays symbolic, without attempting
to create the corresponding millions of tier0 facts.

Still outside this operation: general lazy tier0 fact virtualization, arbitrary
fractal induction, automatic coarse/multi-parent family discovery, GPU cost
models and FlashAttention search.

## Recorded result

`results.json` records three release runs on the same captured held-out workload.
Four training samples merged into two families, each supported by two samples.
Fixed use covered the same 90 of 93 eligible events as full discovery, with three
initialization/context events outside those families. Pending outlets appeared
before their later applications. The fixed-matching median was 8.13 ms versus
120.25 ms for full metadata construction/discovery (about 14.8x for these stages).

The depth-20 binary frontier has 1,048,576 values and sum 12644383195136. Its value
query used zero tier0 applications and zero element/prefix expansion. The
3,145,725 basic applies needed by a full isolated A→2B→A unfolding are a derived
count, not a measured run of that large graph. Small depth results were checked
against actual tier0 enumeration. These results do not establish a general
program speedup or a FlashAttention implementation.

# Do only a few saturated instances need new work?

Measured the current fixed 13-trigger executable library on an **eight-round math
prefix**. This is a diagnostic run, not a speed benchmark. The ordinary execution
path does not collect this audit unless `EGG_LAYOUT_AUDIT_ENCLOSURES=1`.

## Observations

Excluding the first round (no prior instances), **5,936 matches** read facts wholly
contained in one previously instantiated body. Only **8** were associated with a
committed Math insert/update or nonredundant union: **99.865% had no such effect**.
All eight were in the factorization macro (`adaptive_baked_10`), and all their
containing bodies carried an alias/rekey tag. This is an association, not a complete
causal explanation of each match. There were zero incomplete keyed witnesses and
zero effectful source IDs missing from the corresponding match batch.

The strong claim that almost no instances change is not established:

| Round | Prior unique instances | Body output regions grew | Output or lower-input regions grew | Fraction |
|---|---:|---:|---:|---:|
| 2 | 19 | 6 | 6 | 31.6% |
| 3 | 46 | 16 | 17 | 37.0% |
| 4 | 106 | 44 | 50 | 47.2% |
| 5 | 287 | 128 | 133 | 46.3% |
| 6 | 661 | 121 | 130 | 19.7% |
| 7 | 1,052 | 232 | 247 | 23.5% |
| 8 | 1,944 | 777 | 818 | 42.1% |

This measures a **potential invalidation surface**, not the minimum number of
instances that must be reripened. Instances overlap and can share the same eclass.
A region can gain an irrelevant alternative, or an already-saturated new body may
cover the extension. Conversely, direct region growth is not an exhaustive index
of dependencies arbitrarily deep below an input.

There were also 6,272 boundary matches; the exact breakdown is in the raw
report and CSV. Boundary categories are cross-prior-body plus external-fact reads.
Across rounds 2–8, 3,316 boundary matches had committed effects. Thus stability of
internal derivations does not imply that cross-body joins are rare.

## What is counted

- A body instance is a set of actual canonical constructor facts emitted by a
  closure macro, even when storage admission leaves it explicit. Identical fact
  sets are deduplicated; alpha-similar bodies at different bindings remain distinct.
- Each step freezes the membership index **before** execution. Real native keyed
  LHS read events are classified as inside one prior body, across prior bodies,
  involving unowned facts, or entirely outside the recorded facts. No current-step
  body retroactively explains its own producer. Outside-owned-facts does not imply
  graph disconnection: a new row may share an existing boundary eclass.
- Body output regions are global eclasses that are results of body rows. Lower-only
  regions are referenced Math classes without a result row in the body. We compare
  live constructor content before/after, canonicalizing old facts through unions.
  Changes from source actions between schedules are also detected.
- Reading a body does not count as changing it. Productive boundary participation
  and actual region growth are reported separately. The former counts all possible
  overlapping owners, not an optimal ownership assignment.
- Logical matches are not writes. Private binding notification writes are excluded.
  Physical commits can subsequently merge/deduplicate; effectful-match counts are
  not counts of distinct surviving new e-nodes.
- Aliasing is checked at instantiation and retained when later unions identify
  previously distinct body values. Symbolic saturation is not guaranteed to remain
  complete under a non-injective binding.

## Controls and verification

Five controlled regressions passed:

1. One external graft reads 1 of 12 bodies.
2. Grafting across all 12 reads all 12, but changes none of their output regions.
3. An external union enables a previously unavailable cross-boundary match.
4. Unioning an extra constructor into one existing body changes exactly 1 of 12
   output regions, including detection of the change between schedules.
5. `A(x) -> B(x)` is locally saturated with opaque x. Adding `x = A(x)` can enable
   `A(A(x)) -> Done(x)` entirely inside its image. The alias tag catches this case.

Eight existing adaptive-runtime regressions also pass. The instrumented and normal
runs have identical per-round e-node/e-class counts. Match counts agree in rounds
1–7; tracing records two extra matches in round 8 (7,615 vs 7,613). This difference
is retained, not hidden. **Final graphs are exactly equal**, not merely equal size:
all 15,493 Math constructor facts and the partition into 5,150 eclasses match under
a bijection constructed from ground constructor keys and decoded primitive values.
The verifier rejects ungrounded cycles and inconsistent mappings; it does not guess
an isomorphism. This comparison is for the supported ground Math/i64/String domain.

The result supports suppressing certified redundant internal work, with alias and
boundary handling. It does not justify assuming a tiny global dirty set, nor prove
the same rates for a different library or a better/larger enclosure partition.

## Reproduce

```sh
cargo build --release --offline --example adaptive_math
python3 tools/bench/enclosure_audit.py --output out/locality-new
cargo test --offline --test reuse_enclosure_audit --test reuse_adaptive
```

The runner copies the mini source and explicitly changes its schedule to eight
rounds. It uses independent processes with an external 30-second/4-GiB guard.
`report.json`, `rounds.csv` and `index.html` contain results. Final graph snapshots
are verification artifacts in the output directory, not full trace histories and
not committed to the repository. Raw event batches are drained after each step.

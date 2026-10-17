# Fact-driven specialization of whole Cell transitions

The previous runtime reused an exact transition only for the same two concrete
Cell handles. This component also reuses a transition for **new input identities**
when the parent templates and complete input-equality pattern are unchanged.

It learns from an actual supplied constructor fact after its resulting Cell has
been established. It does not enumerate hypothetical neighbors, alias partitions,
or arbitrary combined rules.

## The learned object

```text
fixed rule environment
+ outer constructor
+ left/right Cell templates
+ equality partition of both parents' input slots
    -> target Cell template + source-slot projection
```

For example:

```text
left:  (a+b)*x
right: (a+c)*x
outer fact: Add(left,right)

guard: L0=R0, L2=R2, with all remaining groups distinct
target parameters: [L0,L1,L2,R1]
```

The target describes the **entire** saturated context. It retains the parents'
facts as well as any generated factorizations. It is not merely the root RHS.

The recipe contains no concrete input names, input IDs, or source instance IDs.
The same guarded recipe applies to a renamed pair such as `(d+e)*z`, `(d+f)*z`.
New equalities change the guard and may require a different specialization.

The guard is deliberately exact: its different groups must currently denote
different opaque input identities. This does not assert mathematical inequality
for all time. Input unions re-canonicalize those identities before a later lookup.

## Why transfer is valid

This proof is restricted to an immutable, positive constructor-rewrite environment
whose rules cannot inspect the names or primitive contents of opaque inputs.
The specialization component checks that language restriction. Guards, literals,
functions, relation effects, subsume, nondefault rule environments, and mutable
analysis data require a separate contract and are not admitted by this proof.

Two instances of the same parent templates and equality partition are related by
a bijective renaming of their distinct opaque inputs. Every rewrite step in this
restricted language is preserved by that renaming. Therefore, for a finite closed
state, renaming commutes with saturation, up to e-class identities.

The first observation additionally checks an independent symbolic context:

1. Construct the exact external constructor over both complete parent seeds.
2. Name its distinct inputs with canonical symbolic slots.
3. Native-Bake that fact context, reusing an existing seed entry when possible.
4. Compare its rooted whole closure with the proposed result under the same
   symbolic input map.
5. Record the target template and source-slot projection only if they are equal.

A root-equivalent but effect-incomplete result is rejected. For example, the
RHS-only power-combination state cannot be certified as the full transition.
The stored hash identifies the rule environment; hash equality alone does not
certify state equality. Closure comparison still uses exact structural keys.

## Runtime behavior

`specializations.py` owns the guard, proof and projection. `Runtime.drain()` tries
the existing concrete-handle cache first, then this parameterized fact cache.
A hit classifies the parent inputs, copies the target slots, interns the target,
and publishes it to its consumers. There is no internal pattern matching,
matrix evaluation or native saturation on this hit path.

This path is not free: the guard is classified on the CPU in O(number of input
slots), output slots are copied, and the existing exact whole-closure instance
interning still runs. The latter can be expensive. We do not claim zero total
update cost, end-to-end speedup, or an improvement caused by GPU throughput.

A miss uses the prior numerical/native path. Only the completed factual context
creates a recipe. The same native fallback context can also be learned when no
compiled rule matched; a constructor fact does not disappear just because its
current optimization rules fail to apply.

Rules and template IDs are scoped to the in-memory library. Reports serialize
the evidence for inspection; loading these recipes into an unrelated library or
persisting/rebinding them across sessions is not implemented here.

## Ablation

The experiment uses four phases with the same structural workload:

1. Cold input identities 0…44.
2. The same structures with entirely new identities, offset by 100.
3. A new input union in that second workload, changing fact guards.
4. Another fresh workload, offset by 1000, with the refined equality introduced
   before execution.

All variants retain the previous template cache, alias repair and exact Cell
interning. The baseline disables only parameterized fact-transition reuse.
This disabled variant is a control in the same supported environment, not an
extension to guarded or effectful rule sets.

| Phase | Baseline matrix batches | Fact specialization | Baseline Bake | Fact specialization Bake |
| --- | ---: | ---: | ---: | ---: |
| Cold | 5 | 5 | 22 | 23 |
| New identities | 5 | 0 | 0 | 0 |
| New equality | 1 | 1 | 11 | 11 |
| Transferred equality | 5 | 0 | 0 | 0 |

Each renamed workload uses 13 direct fact transfers; two other transitions are
already reused by the concrete-handle cache. The additional cold Bake is counted
as proof overhead. The new-equality phase creates five new certificates rather
than accepting an invalid old guard.

The baseline's logical MAC counts grow with the width of the new identity IDs.
Comparisons between variants use the same IDs in each phase. No wall-clock
speedup or neural-network throughput comparison is inferred from these counts.

## A simpler cold-path control

`FactOnlyRuntime` in the demo closes each supplied constructor context directly
with native Bake and learns the same proof. Its results are identical, with:

- Cold input: **16 Bake calls**, no matrix batches.
- New equality: **5 Bake calls**, no matrix batches.
- Both renamed workloads: no new Bake, no matrix batches.

This is a significant limitation on claims about the GPU experiment. When the
input already specifies both children and the real outer constructor, cold
boundary matching is not required to know which context to saturate. The gains
here come from factual closure specialization. The value of GPU search where
neighbors or bindings are still unknown needs a separate workload and comparison.
The control is kept explicit; it is not silently substituted into GPU timings.

## Validation and reproduction

All variants compare exact whole-Cell states after every phase. The baseline
also performs 50 native whole-graph checks across initial input, renamed input,
the changed equality, and the transferred equality. The learned Metal/MPS path
is compared to these same phase states. Tests disable the matcher, native Bake,
and vector encoding on a warm replay to verify that none is covertly required.

```sh
python3 experiments/cell_spine/transition_program/specialization_demo.py \
  --output out/fact-specialization-next

python3 -m unittest experiments.cell_spine.transition_program.test_specializations -v
```

Use `--skip-build` after building native examples, and `--cpu-only` without Metal.
The directory must be new. The generated page displays guards, target templates,
slot projections, replay counts and the direct-fact control. Large debug exports
remain untracked; only a compact result summary is committed.

No weakest-guard discovery, arbitrary conditional-effect lowering, global hidden
e-class union protocol, or persistent GPU service is claimed. These recipes
transport exact already-proved contexts; they do not prove unseen structural
families are covered.

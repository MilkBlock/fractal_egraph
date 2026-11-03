# Native CPU multi-stage validation

These tests execute the actual local egglog engine. They do not benchmark a
standalone Python model, a GPU worker, or replay of a previously saturated graph.
The ordinary control is the **same binary and local kernel** with Cell execution
disabled; it is not the immutable pristine `egglog-baseline` import.

```sh
python3 experiments/cpu_multistage/check.py --output out/cpu-check
python3 -m unittest eggfuse.ir.test_recipe eggfuse.analyzer.test_regions \
  eggfuse.analyzer.test_batches eggfuse.compiler.recipes.test_constructor_witnesses
python3 experiments/cpu_multistage/bench.py --output out/cpu-bench
# Opt into the frozen real Pass 3 cases, including the larger Raytrace input:
python3 experiments/cpu_multistage/bench.py --output out/cpu-pass3 \
  --cases fact loopfact sum-bits raytrace
```

The benchmark makes one Cell/native pair per source, without averaging. Inference
times exclude compiler/bootstrap/export/checker work; those costs are reported
separately. The 28-stage, 1,024-input chain is a deliberately favorable synthetic
workload, not evidence of eggcc-wide speedup. Pass 3 inputs are assembled from
`experiments/profile_suite/suite.json` and checked against their frozen SHA-256.

The regression fixtures cover positive chains, finite Run/repeat, independent
guarded branches, late input and union, hidden witnesses, hidden-plus-union
fallback, preexisting unrelated effect rows, Unit functions, mixed positive
regions, nonpositive observers, correlated multiple bindings, private completion
receipts, deletion/subsumption between activations, custom merges and min updates.
They compare complete typed graphs rather than node counts. A unique extraction
is checked too. Fresh-process replay, new inputs using a saved plan, malformed
wiring and forged source artifacts have explicit checks.

An integration failure found during development is retained in the full suite:
executing an empty native cleanup rule advanced the seminaive epoch and changed
`loopfact`'s order-sensitive `to-hoist :merge new` choice even without an inlined
successor. Auxiliary tables now reset directly with native generation/index
invalidation. They do not create extra source Runs.

The final measurements and binary/source hashes are in `results.json` and
`RESULTS.md`. Full states and verbose execution reports stay under `out/`.

The first eager Raytrace trial executed 1,412 successor invocations but failed
complete endpoint comparison (697,938 versus 697,922 rows, with differences in
Smaller/TCPair/TermConcat). This is not a certified speedup. Native identity and
enumeration changes can be observed by later order-sensitive function merges,
even outside a locally positive region. The default compiler now rejects that
optimization contract and executes such programs natively. Pass 3 coverage
expansion remains future work; the benchmark must report zero Cell credit on
this fallback.

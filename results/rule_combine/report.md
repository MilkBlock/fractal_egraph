# Online rule composition experiment

All modes execute the real local egglog engine. Source rules remain installed. Shortcuts are sound two-step compositions of unconditional constructor rewrites; composition is independent of observed trace frequency.

|case|roots/wave|waves|mode|rounds|enabled|inference ms|discovery ms|total ms|retained KiB|
|---|---:|---:|---|---:|---:|---:|---:|---:|---:|
|algebra|64|1|dynamic|3|3|1.916|0.705|5.670|2088.46|
|algebra|64|1|observe|3|0|1.353|0.690|4.970|2011.64|
|algebra|64|1|original|3|0|0.542|0.000|4.073|2002.56|
|algebra|64|1|static|3|6|0.952|0.000|5.224|2156.12|
|algebra|256|4|dynamic|12|3|28.067|9.245|67.916|5134.21|
|algebra|256|4|observe|12|0|18.546|9.400|56.983|3579.00|
|algebra|256|4|original|12|0|5.087|0.000|44.439|3569.93|
|algebra|256|4|static|12|6|15.183|0.000|56.876|5798.28|
|chain|64|1|dynamic|9|7|1.305|0.470|3.699|2048.76|
|chain|64|1|observe|9|0|0.891|0.458|3.247|2001.12|
|chain|64|1|original|9|0|0.187|0.000|2.385|1993.41|
|chain|64|1|static|5|7|0.264|0.000|2.940|2047.85|
|chain|256|4|dynamic|24|7|5.490|1.682|22.091|2694.95|
|chain|256|4|observe|36|0|4.087|1.646|21.013|2525.78|
|chain|256|4|original|36|0|1.537|0.000|18.057|2518.06|
|chain|256|4|static|20|7|2.375|0.000|19.340|2689.04|
|negative|64|1|dynamic|1|0|0.008|0.000|2.629|1520.22|
|negative|64|1|observe|1|0|0.008|0.000|2.590|1520.22|
|negative|64|1|original|1|0|0.008|0.000|2.578|1513.26|
|negative|64|1|static|1|6|0.009|0.000|3.039|1579.89|
|negative|256|4|dynamic|4|0|0.043|0.000|22.306|1777.34|
|negative|256|4|observe|4|0|0.036|0.000|22.426|1777.34|
|negative|256|4|original|4|0|0.034|0.000|22.140|1770.38|
|negative|256|4|static|4|6|0.061|0.000|23.320|1837.01|

## Semantics and controls

- original: source rules only, no tracing or composition preprocessing. observe: preprocess and run the same bounded discovery, including virtual selection and stopping, but install nothing. static: install every preprocessed candidate before inference. dynamic: install a candidate after two observations, at most eight active shortcuts; stop tracing/discovery when every candidate or the active budget is installed.
- Preprocessing uses the egglog expression parser, variables renamed apart, first-order unification with occurs checks, and explicit RHS overlap paths. Variable-only overlap positions are excluded. Canonical candidate variables preserve sharing across both sides and the intermediate witness. RHS variables must occur in the LHS. Candidate sides are limited to 32 AST nodes and the catalog to 64 entries.
- Runtime discovery starts from Survived logical matches of source rules. It resolves the first RHS subterm in the current canonical graph, then checks the second LHS against that local class with consistent variable bindings. This is evidence of current applicability, not evidence that the first match committed that subterm. Guards, arbitrary actions, subsumption, primitive side effects and mixed-sort rules are not supported.
- For this prototype a constructor-row index is rebuilt at each discovery round. At most 4096 surviving events and 128 distinct candidate/target pairs are probed per round; repeated candidate/target pairs are skipped. Each local match has a 256-visit budget and at most 16 substitutions. A missed discovery is safe because source rules remain available. Observed logical-match counts are only for traced phases, not comparable total matching work across modes.
- chain is an intentionally synthetic eight-step unary chain; algebra combines distribution, multiplication by zero/one, addition with zero, and double negation; negative uses Add of two variables and triggers none of those source rules. Sources arrive in one or four waves; each wave is saturated before the next.
- Every measured run is compared by complete canonical constructor records AND root labels across modes. Class names are least-size ground representatives, so differences in allocated IDs do not invalidate comparison. This covers the supported single E sort and its Var integer payload; not arbitrary user-defined egglog schemas.
- Six tests cover occurs check, nonlinear patterns, chain and nested compositions, every generated candidate via actual first-rule/second-rule execution, and exact closure equality for three workloads across all modes.
- Inference time includes tracing, discovery and dynamic compilation inside the loop. Static compilation is outside inference time but included in total and install time. Total additionally includes source preparation, engine initialization, input generation/parsing/insertion and preprocessing. Memory includes the engine, roots, candidate catalog, counters and activation records. Peak RSS also includes post-measurement closure verification.

```sh
cargo test --bin rule_combine
cargo build --release --bin rule_combine
python3 experiments/rule_combine/run.py
```

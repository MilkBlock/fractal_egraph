# Prefix witness ablation 1

Actual local egglog, one source step. Prefix catalog: Add(p0,p1), Add(Mul(x,y),Neg(x)), and Add(Neg(x),Mul(x,y)). Prefix instances include canonical root IDs and ordered boundary bindings, including repeated x.

|case|unrelated roots|mode|source + recognition ms|recognition ms|found|global catalog instances|
|---|---:|---|---:|---:|---:|---:|
|fresh|0|global_rematch|0.771|0.568|512|512|
|fresh|0|rematch|0.876|0.683|512|512|
|fresh|0|source_only|0.206|0.000|0|512|
|fresh|0|witness|0.329|0.104|512|512|
|fresh|4096|global_rematch|7.766|6.267|4608|4608|
|fresh|4096|rematch|5.939|4.121|512|4608|
|fresh|4096|source_only|1.799|0.000|0|4608|
|fresh|4096|witness|1.754|0.109|512|4608|
|inactive|4096|global_rematch|2.797|2.792|4096|4096|
|inactive|4096|rematch|0.014|0.009|0|4096|
|inactive|4096|source_only|0.005|0.000|0|4096|
|inactive|4096|witness|0.006|0.001|0|4096|
|preexisting|0|global_rematch|1.162|0.822|768|768|
|preexisting|0|rematch|0.973|0.686|512|768|
|preexisting|0|source_only|0.250|0.000|0|768|
|preexisting|0|witness|0.346|0.095|512|768|
|redundant|0|global_rematch|0.796|0.582|512|512|
|redundant|0|rematch|0.939|0.717|512|512|
|redundant|0|source_only|0.222|0.000|0|512|
|redundant|0|witness|0.335|0.098|512|512|

## Scope and controls

- source_only runs the source rule without a detector; it is a timing floor, not a recognition algorithm. global_rematch runs native indexed prefix queries over the graph. rematch uses native queries anchored by a pre-established Target(root) relation, an intentionally strong baseline given the active roots. witness reuses source trace substitutions and verifies constructors by keyed lookup after the step.
- Native detector queries are implemented as redundant union rules with trace output because the public API lacks a query-only substitution sink. We assert updated=false for each detector. This includes action/trace overhead, so it is not a lower bound for an optimized query-only implementation.
- All detector rules and Target relations are prepared in every mode before timing. Setup costs are excluded. Measured time includes the actual source step and extra source tracing in witness mode, plus event handling, verification lookups and result-set allocation. It is not only the hash lookup cost.
- Witness mode gives rule-derivation prefix instances, not all prefixes in the graph. The pure-swap fixtures have no extra alternatives in active classes, so the anchored native baseline and witness result sets are exactly equal. Global queries additionally find preexisting RHS-only regions and unrelated Add roots. A separate test demonstrates that another alternative in the same active class can also escape the witness.
- Scope is a single unconditional, non-deleting E-sort rewrite with x and y retained by the existing logical trace. This does not provide generic physical LHS row witnesses or per-mutation provenance. Successful source matching certifies the LHS; keyed post-step lookups confirm the RHS exists, including the case where it already existed.
- Five independent release processes per configuration, 100 runs total. Phase memory tracks allocation-request delta relative to the prepared graph, not whole-engine compression. No claim about production routing or memory reduction is made.
- Future ablations: incremental prefix lifecycle across merges and alternative additions; then exact-set history versus counted history versus Zobrist history, with field/index/update costs included. Do not interpret equal history hashes as structural equality.

```sh
cargo test --bin prefix_witness
cargo build --release --bin prefix_witness
python3 experiments/prefix_witness/run.py
```

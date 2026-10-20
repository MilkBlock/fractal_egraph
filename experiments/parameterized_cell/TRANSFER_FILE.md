# Analyze -> transfer file -> fresh runtime

The multi-producer example can now be exported and executed without retaining
the original training source, a captured graph, or a history file.

Two export routes use the same native Analyze capture:

```sh
# Normal analysis, plus a transfer file in the analysis directory:
cargo run --release --bin egg_layout -- analyze \
  --source training.egg --output out/analysis --bake-transfers

# Focused capture/export, without ripen discovery, graphs or a full history file:
cargo run --release --bin egg_layout -- analyze-transfer \
  training.egg out/transfers.egg

# Fresh runtime: only the library and the new facts/checks/schedules are inputs.
cargo run --release --bin egg_layout -- run-transfer \
  out/transfers.egg updates.egg out/cold
```

For this example, `training.egg` is the concatenation of `terms.egg` and
`competing_updates.egg`; `updates.egg` can be `competing_updates.egg` itself.
The output directory and transfer target must be new. Ordinary `analyze`
does not export transfers unless requested. Unsupported export contracts report
an error rather than discarding rules.

## Contents and execution

The file is valid egglog syntax: native datatypes, functions (including merge)
and rules are ordinary source. JSON annotations in comments carry:

- `@cell-transfer`: format version, selected rulesets and observed schedules;
- `@cell-program`: seed atom, fixed key/output read order, typed slot wiring,
  constants and every guard;
- `@cell-evidence`: aggregate committed-history observations and producer/read
  connections. No training values or graph instances are copied.

The offline exporter currently recognizes the checked three-ruleset terms
family. It uses the captured rules and actual producer/read witnesses; it
includes unobserved source branches too, such as the negative-cost panic.
Trace observations alone are not treated as universal proof: the complete
query wiring is checked against the native typed atoms.

Loading installs the embedded schema and native action sinks, checks their
shape/guards against the file, and verifies that the supplied read order binds
every atom exactly once. It does not search for a seed or access plan and does
not call Analyze or the terms-specific compiler. Runtime values are interned
in the new engine; the file contains no native Value/Table/Rule IDs.
Compiler-generated slot labels appear only in JSON comments, where egglog's
reserved-symbol parser cannot interpret them as source variables.

The normal parameterized runtime then follows these programs, keeps native
Run boundaries, applies the native RHS/merge/rebuild and retains all aliases.
`program_compilations=0` means **no port-plan discovery**, not no native code
installation. This version loads the complete port-dataflow program, not the
separate fresh-DAG macro or the scoped dominated-candidate optimization.

## Verification

`tests/transfer_artifact.rs`, included in the existing benchmark skill's `test`
mode, verifies:

1. Every update boundary against the native same-submit CPU's complete original
   rows, values, equality partition and visibility.
2. A previously unseen `TermLeaf(200)` candidate of cost zero.
3. Renaming all rulesets after export: loading still works, demonstrating that
   the loader does not rerun a hard-coded terms compiler.
4. Rejection of missing/duplicate atoms, invalid seeds, changed guards,
   mismatched rulesets, unknown versions and embedded training actions.
5. A separate analyzer process, deletion of its training source, then a fresh
   runtime process using only the library and input changes.
6. Export from normal `analyze --bake-transfers`, reusing its existing capture
   without requiring `history.json`.
7. The negative-cost branch, never taken during training, still raises the
   native error after loading and prevents resuming the failed runtime.

Actual saved demonstration: `out/analyze-transfer-loaded-v1`.
It exported seven programs in a 13,271-byte file. Analyze retained 98 records,
44 with committed changes, and 46 distinct producer/read connection shapes.
Its existing capture also rejected 137 records; these are reported, not hidden
or used as evidence that unobserved branches are impossible.

The fresh process began with zero mirrored facts, loaded seven programs,
performed zero plan-discovery compilations and zero ordinary native fallback
runs, and completed eight boundaries (the seven original phases plus the
unseen update). Native effects still execute; this is not a GPU benchmark or a
Pass-3 speedup result. The checked artifact is
[results/transfers.egg](results/transfers.egg).

This is an end-to-end Bake artifact for the admitted parameterized terms
program. It is not a universal automatic Cell-family discovery algorithm,
a termination proof, or a summary which erases intermediate helper effects.

## First release timing comparison

`bench.py transfer-bench --output out/NEW` runs the existing artifact, without
re-Bake or a GPU worker. The fixture is expanded to 512 independent instances
using egglog AST traversal: only V/TermLeaf identities change, not costs or
guards. Each of the seven update phases is executed once per lane. Input
generation, checks, full-graph comparison, serialization, drop and compilation
are outside timing; parsing/committing incoming facts is measured separately.

| Lane | Setup ms | Input ms | Inference ms | Sum ms |
|---|---:|---:|---:|---:|
| Source-compiled port dataflow | 3.013 | 534.574 | 181.164 | 718.751 |
| Loaded port dataflow | 2.630 | 532.263 | 185.200 | 720.093 |
| Existing source joint path | 3.770 | 526.821 | 197.683 | 728.274 |

Loaded versus the equivalent source dataflow is +0.19% in the sum and +2.23%
in inference. This single measurement shows no large aggregate regression; it
does not establish a statistical bound or prove that the inference difference
is noise. Both perform 293,376 port-index reads and submit 51,712 bindings.
The joint path has additional certificates and skips some no-op work; it is
not a pure serialization control.

All seven boundaries match the native same-submit query reference as complete
public graphs (rows, literal values, equality and visibility). The final graph
has 56,836 public rows, including functions as well as constructors. Auxiliary
shared-row handle counts are 112,128 for both dataflow variants versus 50,688
for joint. These are counts, not RSS measurements.

Evidence: `out/transfer-bench-512-v1`, retained as
[results/transfer-bench512.json](results/transfer-bench512.json).
This measures the actual CPU runtime on the small terms family, not full
raytrace or a new GPU speedup. The historical full Pass-3 measurements were
not repeated or relabeled as this file-loading experiment.

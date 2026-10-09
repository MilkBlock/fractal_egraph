# Actual Cell closures → exact scalar operators

This experiment uses **saved eggcc Cells**, their recorded parent connections,
and their original rules. It extracts an affine `Expr-size` subsystem and emits
one executable egglog rule. It does not change the execution engine or benchmark
performance. It is a standalone compiler experiment with native runtime checks,
not a production Cell dispatcher.

## The actual family

The inputs are completed observations from
`out/eggcc-full-replay-20261004-stack16m/run/saturated-rule-composition/cells`.
Cell IDs select this experiment; coefficients and formulas are derived from the
source AST and snapshot, not selected by ID.

```text
1729: Const c1 ─────────────┐
                          ↓
opaque Expr x ───→ 1865: Bop(op1, x, c1)
                          ↓
1730: Const c2 ──→ 1893: Bop(op2, Cell1865.root, c2)
                          ↓
                  1916: Uop(op3, Cell1893.root)
```

The saved source defines `size(Const)=1`, `size(Bop)=size(x)+size(y)+1`,
`size(Uop)=size(x)+1`, and merges size by `min`. These rules are read using
egglog's parser. The implementation does not hardcode those constructor formulas.

Let `s` be the initially absent size of the opaque boundary expression:

| Cell | Recorded parents | Derived root size |
| --- | --- | --- |
| 1729 | none | 1 |
| 1730 | none | 1 |
| 1865 | 1729 | s + 2 |
| 1893 | 1730, 1865 | s + 4 |
| 1916 | 1893 | s + 5 |

Typed history tokens identify corresponding ports. For example, the same
boundary `Expr:1767` is `(RipenInput_5_ 4)` in Cell 1865 but
`(RipenInput_5_ 5)` in Cells 1893/1916. Literal slot-number equality would wire
these incorrectly. Roots are selected from the recorded final injection,
not from largest-term or similarity heuristics.

## Where linear algebra is used

1. Derive each source rule's arithmetic map over its input fields.
2. Build a matrix wiring actual parent output ports to child inputs.
3. Multiply the arithmetic matrix by the binding matrix. Retain every parent
   coordinate, even when a child expression cancels it.
4. Check that this composed map equals independent expansion of the child's
   original scalar equations.
5. Use exact rational Gaussian elimination to share the output row space.
   Emit the resulting basis computation and reconstructions as rule actions.

For Cell 1893 the concrete composition is:

```text
[1 1 1] × [1 2] = [1 4]
          [0 1]
          [0 1]
```

The middle matrix supplies the previous root size `s+2`, constant size `1`,
and the homogeneous constant `1`. Across the three roots:

```text
[1 2]   [0 1] × [0 1]
[1 4] = [2 1]   [1 2]
[1 5]   [3 1]
```

Thus only one variable-dependent coordinate is needed: `t=s+2`; the later
root sizes are `t+2` and `t+3`. `2`, `4`, and `5` are actual affine offsets,
not Boolean incidence entries or numerical encodings of e-class identities.
This particular family does not demonstrate repeated-operand coefficients
such as `2*s` or nonlinear polynomial reduction.

The generated [example.egg](example.egg) is the actual Cell-1916 shortcut.
It reads one boundary field, calculates the shared basis once, and writes all
three previously absent size fields. It still resolves constructor keys and
performs native sets/rebuilds. Its structural prerequisites are supplied by
the saved Cell; **do not install it as an unrestricted global-graph rewrite**.

## Verification and limits

[results.json](results.json) records the compact results and input hashes.
The full generated report retains typed binding maps and source-rule witnesses.

For each of Cells 1865, 1893 and 1916, the native verifier starts from the same
saved closure and tries eight boundary sizes, including `i64::MAX - 5`:

- Original rules: inject the boundary size, then saturate the saved rulesets.
- Direct operator: inject the size and write the computed fields without
  executing any original rule.
- Generated rule: inject the size and execute the shared-basis shortcut once.

All **24** cases agree on **all 355 tables**, including subsumed flags and
canonical values. All non-size tables remain unchanged, as do frozen parents.
Each shortcut executes exactly one rule match. Matches and mutations are not
equated. No wall-clock speedup is claimed.

Every original arithmetic subtree contributes a definedness constraint;
algebraic cancellation cannot conceal an overflowing intermediate. This family
uses `0 <= s <= i64::MAX - 5`. Repeated bindings that are equality guards,
nontrivial `min` envelopes, nonlinear products and cycles are rejected.
Rational/negative basis reconstructions are not compiled to i64 operations.
Rules potentially affected by size but omitted from the extracted equations
receive conservative absence checks on full native tables, not just the
container-limited observation export. A wrong affine effect and an out-of-domain
input are independently rejected by the native verifier.

The saved graph exports remain **observation-only** because their original
container exporter was incomplete. They are not upgraded to Cell-equivalence
or union certificates. The arithmetic identity is exact for the extracted
equations; native all-table agreement is a **sampled** check in this fixed
structural/ruleset environment. General union/subsume transitions, min-plus
fixed points, changing Cell structure and arbitrary rewrite algebra remain
outside this experiment. Matrix equality of size fields does not imply Cell
equivalence.

## Reproduce

The saved input directories above must exist, or supply `--cells PATH` with
the same real observations. There is no synthetic fallback and no analyze run.
The native verifier uses the local modified egglog runtime, not pristine
upstream; the report records binary hashes and the kernel-diff hash.

```sh
cargo build --offline --release --example lhs_ast --example cell_operator_verify
python3 experiments/real_cell_operator/run.py
python3 -m unittest discover -s experiments/real_cell_operator -p 'test_*.py'
```

Generated files live under `out/real-cell-operators/`: `report.json`,
`manifest.json`, `native.json`, and each Cell's `operator.egg` and
`isolation.egg`. No large history stream is generated.

Code: `artifacts.py` validates real inputs; `derive.py` extracts scalar
equations; `algebra.py` composes and factors matrices; `run.py` ties them to
native parsing and verification; `examples/cell_operator_verify.rs` executes
the three paths. Unit tests exercise algebra/rejection logic, not invented
Cell workloads.

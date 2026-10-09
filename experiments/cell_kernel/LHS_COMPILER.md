# From the actual CICM LHS to a shared typed relation

2026-10-09. This step removes the manually supplied fragment boundaries for the
CICM query. It is a source compiler plus same-snapshot native equivalence test,
not a benchmark, full compiler optimization, or automatic production deployment.

## Reproduce

```sh
cargo build --offline --release --example lhs_ast --example lhs_equivalence
python3 experiments/cell_kernel/verify_lhs.py
python3 experiments/cell_kernel/test_lhs_compiler.py
```

`lhs_ast` calls the existing egglog parser; no new .egg parser is implemented.
It exports surface rule ASTs and declaration signatures without executing code.
The compiler infers variable sorts from these signatures and literal types.
Unknown/ambiguous calls or sorts are rejected. Parser-internal @ names, globals
and generated-name collisions are outside this pilot rather than printed back
as invalid or ambiguously scoped programs.

## Automatic structural decomposition

1. Find repeated constructor bindings of the form `(= v (Ctor ...))` with
   different named roots. Constructor names come from the parsed schema; Uop,
   e1 and e2 are not hardcoded into the compiler.
2. Assign a whole source conjunct to a root if it mentions exactly that one root.
   Zero-root and multi-root conjuncts remain in the residual query.
3. Alpha-normalize each group. Boolean literals in declared table/constructor
   argument positions become explicit exported parameters. Guard constants such
   as 10 remain literal; they are not replaced by unbound variables.
4. Reuse identical normalized templates. Export every source named variable in
   the fragment so no joint binding correlation is discarded.

This is a deterministic structural rule, not an optimal cut or semantic-equality
search. Nested calls/guards remain whole source facts. Splitting a nested guard
into separate source conjuncts can change the chosen cut. Source order is retained;
reordered equivalent groups may produce two templates rather than one. These
limitations affect reuse, not the partition's snapshot equivalence.

## Actual result

The original CICM Uop LHS has 18 source conjuncts. The algorithm discovers two
roots, owns six conjuncts per root, and emits one shared nine-field relation:

```egg
(relation AutoTouch_cell_0
  (Expr UnaryOp Expr BaseType Term i64 bool Expr Expr))
```

Its fields are expression, unary op, operand, base type, extracted Term, cost,
branch flag, predicate and original inputs. Its producer includes the original
Uop, HasType, ExtractedExpr/TCPair, strict size<10, purity and ContextOf conditions.

The rewritten consumer has eight source conjuncts:

```egg
(rule ((= if_e (If pred orig_ins thn els))
       (HasArgType thn (TupleT tylist))
       (HasArgType els (TupleT tylist))
       (ContextOf if_e outer_ctx)
       (AutoTouch_cell_0 e1 o x ty t1 c1 true pred orig_ins)
       (AutoTouch_cell_0 e2 o y ty t2 c2 false pred orig_ins)
       (= t1 t2)
       (= orig_ins_len (TypeList-length tylist)))
      (... original actions ...))
```

All 18 named binding fields are retained, including x/y, t1/t2 and c1/c2. There is
no added e1!=e2 condition and no requirement that extraction costs be equal.
Eight source facts is NOT a measurement of eight Free Join probes or stages.

Inspectable files in lhs_example/:

* generated.egg: producer and rewritten binding collector.
* rewritten-rule.egg: producer and transformed rule with original RHS actions
  preserved by the native AST printer; not executed by the binding-only test.

Both require the original schema/current facts. Original and replacement rules
are not intended to be installed together in production. The verifier alone
runs both collectors to compare their answers.

## Why this replacement is exact at a snapshot

Partition B into B0, B1, B2, with exported variables containing every named
variable used by each fragment. Define Vi as the native answer relation of Bi.
Instantiating the lifted literal parameters reproduces the original fragment
exactly. Consequently:

    Answers_D(B0 AND B1 AND B2) = Answers_D(B0 AND V1 AND V2)

The proof is conjunction decomposition and reconstruction of all shared bindings.
It does not depend on constructor injectivity, unique enodes per e-class, or
functional uniqueness of relation rows. The tests also reconstruct the original
AST conjuncts from template calls and verify a disjoint, complete partition.

## Native checks

The same EGraph replays 20 event stages. After each event it builds fresh view
relations, runs original/rewritten collectors, and immediately compares the full
18-column typed canonical binding sets BEFORE any later event can merge them.

Events include strict threshold9/10, unknown size, min updates, purity, delayed
union/commit, Term/context/operand union, old-node subsumption, the same Expr on
both branches, multiple Uop enodes in one e-class, and multiple TCPair cost
witnesses in one function output e-class.

The merge path is also exercised: setting ExtractedExpr constructs an unresolved
Smaller and removes some matches; unioning it with the chosen TCPair restores
them. A second query adds `c1<=c2`; that cross-side guard stays residual, and it
reuses the same normalized local template.

All 40 native set comparisons passed, plus 13 source-compiler tests. This checks
full binding tuples, not just total counts or a few expected matches. Source,
parser-binary and oracle-binary hashes are in lhs-checks.json. No trace capture,
analyze or performance measurement is performed.

## What still costs work

The emitted producer STILL executes the original six-condition query. This is
not a claim that total search vanished. The new capability is automatic source
recognition and a verified consumer replacement. Direct publication of the same
row by an existing Cell program/history witness is the next integration step;
then the producer query could be bypassed for those certified instances.

Fresh per-stage relations prevent stale matches after union/function updates or
subsume. This is a validation protocol, not scalable incremental maintenance:
production code must add an epoch/maintenance discipline instead of retaining
unbounded versions. Binding equality does not prove that running old/new RHS in
different schedules has identical history. The original RHS's action sequence is
preserved textually; full CICM transformation and the previously found local
fresh-create/subsume issue are not certified or repaired by this change.

The scalar Program/touch machinery remains useful for compiled numeric components;
this compiler adds a typed relational fragment representation for genuine LHS
multiway matching. It does not pretend an arbitrary query is a scalar DAG.

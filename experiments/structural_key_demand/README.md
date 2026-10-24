# Consumer-driven structural keys

This replaces unconditional key generation with source-declared uses, backward
schema liveness, and runtime candidate demand. No eggcc table/rule name appears
in the algorithm. A selection suggestion is not authority to change a merge.

## Source interface

Without a declaration, `:merge old/new` remains unchanged and creates no key
readers. If there are no readers, the output has no key tables, helper rules or
maintenance schedules. For already-expanded input the original body is copied
byte-for-byte, and the empty extension header registers no primitive or sort.

To explicitly replace arrival-order selection with scored structural selection:

```egg
; cell-choice/v1 {"function":"winner","score":"rank","order":"max","tie_break":"structural"}
```

`score` is an optional unary i64 function; `order` is `min` or `max`. Without a
score, structural order alone selects the result. This changes the declared
selection policy, so the original arrival-order program is not its oracle.
The score function must be declared before its selector. Neither its name nor
its result sort is fixed. Renamed programs and multiple selectors are tested.

The analyzer reports old/new reference selectors and recognizes the syntactic
`score(v) = best(keys)` pattern where `best` uses primitive min/max. These are
suggestions only. On the actual Pass-3 input it discovers one candidate,
`to-hoist`, its `Expr-size` score and `to-hoist-size` maximum, without name tests.
The only writer ruleset is `boundary-analysis`.

## What is retained

1. Only explicitly selected, reachable writers create key roots. Follow each
   root sort's constructor fields with a worklist. Recursive sorts are visited
   once. A needed unsupported field rejects compilation instead of leaving an
   undefined key lookup. Unrelated types get no key declarations or rules.
2. A source-generated request ruleset reuses each writer's complete LHS to
   request keys for its actual candidates. Key computation follows those
   candidates' constructor children. Existing selected values retain keys, so
   a union of selector keys can merge old results without another discovery
   pass. Unused nodes of the *same* sort also avoid key materialization.
3. Before the original Run: publish requests, saturate the private key rules,
   validate readiness, then execute the original ruleset. Combined rulesets
   request all member writers before their shared frozen frontier. Ordinary
   unrelated rulesets receive no maintenance. Top-level writes evaluate their
   operands once, prepare the required keys, then perform the write.
4. Readiness uses a monotone boolean flag, initialized false and set true only
   after the key and optional score exist. If there is neither a cached key nor an available finite witness (for example,
   a first demand for fully subsumed input), or no score, selection fails first. Missing keys never silently filter out
   an original match. Deleting/subsuming a selected score is rejected.

The request Join is a real additional cost; it is reported, not called free.
No candidate budget, representative sampling, or solver is used for liveness.
Type dependency marking is O(V+E) after constructing the schema graph. This is
not a bound on all native joins or on recursive runtime saturation.

## Stable witnesses, not whole-eclass canonical forms

A key stores a finite constructor witness. Once first completely computed, it
is anchored; subsequent alternatives do not recursively rename that witness.
Union coalesces established keys by minimum. This preserves a stable historical
representation, not the minimum representation of a continuously evolving
whole e-class. Keys are not used to assert original e-class equality.

This distinction mattered on Pass 3: continually improving keys required 2,016
helper Runs and 16.67 s. Anchoring reduced key computation to 1.62 s. That change
also changes some tie outcomes, so the two full runtimes are **not** advertised
as a same-semantics speedup. The final anchored program is checked against its
own same-submit CPU reference, including the key values and auxiliary tables.

`cell-key-select` is a generic typed conditional: both values and its result
must have the same sort, and the condition must be boolean. It avoids constructing
a temporary Map for every merge or creating ambiguous Map sorts for multiple
selectors. This is a source-declared runtime primitive, not an eggcc shortcut.

## Implementation and checks

- `tools/cell_spine/key_demand.py`: discovery, declarations, backward liveness.
- `tools/cell_spine/structural_keys.py`: demand/readiness/choice rules and schedules.
- `cell-assisted-egglog/src/stable_choice/source.rs`: typed conditional and empty-header fast path.
- The native inventory/profiler excludes helper sets declared in the header;
  they still execute but are not offered as application marker candidates.

During validation, duplicate selections exposed an existing nested-merge bug:
when outer old/new were equal, a nested function read returned the outer value
instead of its own result, even when the sorts differed. Commit `4ad40e8` moves
that shortcut to the top-level function merge only. Tests preserve the existing
top-level behavior and exercise nested reads. The immutable baseline tag remains
unchanged; this is the current local kernel, not pristine upstream.

The small `choice.egg` checks selected subtrees, an unrelated node of the same
sort, an unrelated sort, late candidates, rank changes, and union of choice keys.
Additional tests cover renamed functions, multiple selectors, i64 extremes,
recursive type dependencies, combined rulesets, missing evidence, and input/key
unions in one Run. The full source-only marker pipeline is also checked.

## Boundaries

- Candidate requests currently cover all eligible candidates, not only same-score
  collisions. Moving demand inside a lazy collision handler is future work.
- A rule's selected value must already be bound in its LHS. Fresh RHS construction
  is rejected because doing it in a prepass could enable premature original
  matches. Top-level construction is handled explicitly.
- Active naive rules, nested fail schedules/writes, and needed unsupported
  container fields are rejected for opted-in policies. These restrictions do not
  cause keys to be added to ordinary unannotated programs.
- Historical witness keys are distinct from structural equivalence proofs and
  from a global minimum over all later alternatives.
- Full Pass-3 marker profiling was not repeated; this validation isolates the key
  generation stage and its real native/GPU execution.

## Final Pass-3 measurements

These are single executions, not averages. Both default lanes use the same final
binary and worker and agree on all 699,068 original rows and 524,631 mapped
classes. No key sort/primitive registration, table rows or helper Run remains in
the default pruned lane:

| Lane | Inference | Key rows | Key helper time |
|---|---:|---:|---:|
| Original source, GPU | 18.177226 s | 0 | 0 |
| Unannotated source through the pruned SK stage | 18.739206 s | 0 | 0 |
| Explicit declared stable choice, GPU | 18.080170 s | 12,829 | 1.645070 s |

The default pair still has a **3.09% single-run total timing difference**. There
is no extra query/helper work to assign it to; we do not claim a proven zero
regression or rerun until a faster value appears. Earlier normalized-body and
unanchored-key trials are retained in `out/key-demand-pass3`, not selected as
favorable controls.

The explicit policy retains 12 sorts / 107 constructor definitions, materializes
353 candidate requests and 12,829 needed values, and produces 725,432 total rows.
Its entire graph, auxiliary keys and visibility agree with its same-submit CPU
reference (27.563570 s). It is not semantically interchangeable with the original
arrival-order merge or with continuously minimized keys. No ratio across those
policies is reported. Historical eager generation had 524,631 key rows and
15.916260 s of helper work; the reduced footprint/work is reported separately
from whole-program speedup claims.

The final tests include 29 source/choice/pipeline tests (real GPU enabled), 69
bench protocol tests, 11 typed-primitive tests, two merge-function tests and two
typed-selector/empty-header tests. See `results.json` for saved paths, binary
fingerprints and exact comparison results.

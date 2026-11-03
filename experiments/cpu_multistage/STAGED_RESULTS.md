# CPU stage patches — 2026-10-31

This is actual native egglog execution with a new local patch layer. It is not
an isolated-egraph replay or a GPU simulation. Complete native queries still
authorize each patch; the native merger, union-find and rebuild remain the final
authority. No search-skipping or speedup claim is made.

## Semantic checks

34 executable fixtures passed complete fact/value/equivalence/subsumption
comparison with native execution. They include all 21 prior CPU fixtures. The
20 shared Recipe/region tests also passed. Two independent processes reproduced
both complete states and stage audit events. A saved artifact accepted a new
input. Both engines rejected an actually activated failing RHS.

Specific evidence:

- **Visibility:** after A, Q/R tables have zero rows although their patches are
  prepared. An observer inserted before B cannot see Q. After B, Q exists and R
  remains absent; C publishes R. Four successor patches are reused.
- **Same ruleset, different Runs:** two finite Runs produce P then Q, with no R.
  A future patch does not create an extra implicit Run.
- **Repeat:** the real iteration stack is retained; a three-iteration recursive
  input reaches N(3), not N(4).
- **Actual merge reply:** proposing 7 to a min-function already containing 3
  makes the consumer use 3, not the proposal. Its prepared patch is reused.
- **Ordered custom merge:** two updates build the local expression
  `q1=Smaller(p10,p7); q2=Smaller(q1,p5)`. Native commits preserve the actual
  comparison nodes. The consumer reads the real merged result; later comparison
  rules and union/rebuild reach the expected cost-5 candidate.
- **Separate stage ports:** B records a node with value 7; another stage updates
  the function to 1; C reuses its prepared patch with value 1. B's node stays 7.
- **Boundary changes:** union before consumption, union-induced function-key
  collision, late function values, and duplicate constructors across frames all
  preserve the native endpoint.
- **Failure timing:** preparing a future division by zero does not fail a stage
  that never executes it. The activated error is rejected by both engines; this
  does not certify equality of every partial table after an error.

The original premature-publication counterexample involving `:merge new` now
executes with staged patches and agrees with native execution. It is no longer
necessary to reject every such program merely to avoid early visibility.

## Three small real Pass 3 inputs

One new candidate run per frozen input. Unmodified native controls were reused
from `out/cpu-cell-release-bench` and are explicitly **historical**. Input/core
fingerprints and complete endpoints were checked. Source/binary hashes, setup
times and full counters are in `staged-results.json`.

| Input | Historical native inference | Staged CPU inference | Reused successor patches | Final graph |
|---|---:|---:|---:|---|
| fact | 13.436 ms | 21.153 ms | 260 | equal |
| loopfact | 12.756 ms | 21.433 ms | 215 | equal |
| sum-bits | 50.788 ms | 108.404 ms | 868 | equal |

This prototype is slower. It still pays complete native queries and adds frame
preparation, readiness lookups, interpretation and merge-reply reconciliation.
These are identified kinds of extra work, not a measured time breakdown.
The reuse counts are reuse of prepared RHS patches, **not avoided matches or
committed mutations**.

The three cases reached respectively 89/64/199 live frames and
4,637/3,234/10,203 local nodes at peak. Local-node counts are not RSS
or e-node compression metrics. All frames are released by program completion.

The next optimization needs to consume the now-correct staged context to reduce
query or publication work. Making the local frame interpreter faster alone is
not evidence that Cell locality has reduced search.

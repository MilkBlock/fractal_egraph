# Cell publisher findings and lowering corrections

A real encoder-timestamp window covering Runs 5,353–5,356 contains the peak
187,006-binding `always-run` invocation. The publisher reports about 4.12 s;
matching about 0.60 s. The ordinary reference window takes 4.55 s. Counter
intervals can include scheduling/encoder effects and must not be added as a
perfectly disjoint full-program time budget. Complete suffix state/history is
verified against the frozen executor.

The window has 67,383 Cell seeds, 113,322 inline steps, 333,736 Cell effect
requests, including 75,273 constructor-set requests. These are requests, not
committed mutations.

Corrections:

1. A generic typed recipe pass recognizes default-union constructor facts already
   witnessed by the same LHS, following SSA copies and nested witnessed reads.
   Re-reading that constructor uses its known value; setting the same key becomes
   union with that value. Custom merges and unproved key aliases are excluded.
   Checked execution independently validates the keyed row.
2. Single-step recipes retain their library descriptions but execute through the
   existing direct SSA action lowering (`plan=-2`). They do not allocate a
   fictitious multi-stage state or lose the ordinary compiler's context reuse.
3. Actual multi-step recipes execute their temporary SSA slots in thread-local
   storage. A wrapper writes required state back even when an inner step returns
   early, preserving union continuation and error behavior.

The `Smaller`/`TCPair` fixture compares 8,192 witnessed constructor updates with
and without reuse: 0.146568 -> 0.129052 s, exact reference/native state. The 19
regressions pass, including checked union continuations; a separate local/global
SSA comparison preserves all continuation state and counters. The unit pass also
rejects mutable/custom constructor merges and unproved binding aliases.

Complete real Pass 3 with all three changes: **199.035161 seconds**, versus
201.743776 seconds before them. This is a small improvement, not the requested
2x result. The historical 12.34-second reference requires **6.17 seconds**;
the current new backend is still far slower than that reference.

Real evidence lives in:
`/Volumes/Extreme SSD/eggfuse-profile/hier-hot-5353-5356/timings.json` and
`/Volumes/Extreme SSD/eggfuse-profile/localssa-pass3-full/`.

The remaining structural gap is bulk execution of effectful, dependent
multi-stage recipes. Current fusion still validates and publishes these effects
in an ordered scalar owner. More matcher microbench gains do not remove that
serial path. No complete multi-stage coverage or 2x speedup is claimed.

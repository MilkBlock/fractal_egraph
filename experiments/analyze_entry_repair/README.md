# Analyze entry and observed-state repair

This changes the native analyze/ripen path. It does **not** establish held-out
state-machine coverage or runtime Join savings.

## Repairs

- Solve LHS equations to a binding fixed point instead of treating conjunctions
  as source-ordered assignments. Cycles/missing bindings remain errors.
- Separate equation-defined external rows from already-grounded values. Require
  internal rows before injection, but do not require an external row before the
  same stage is allowed to inject it.
- Represent observed external equality-sort function outputs as typed boundary
  slots. Do not invent payloads for unsupported primitive/container inputs.
- Terminate numeric RipenInput sort suffixes with `_`, avoiding collisions with
  egglog's per-name generated numeric variable suffixes.
- Use the source schedule to select local rulesets (including transitive combined
  rulesets). Declared but unscheduled debug deletion rules are not activated.
- For automatically extracted entries, zero-premise applications supply explicit
  seed facts rather than reinstalling the input generator inside every cell.
  This is a **local rule environment**, not a claim to rerun all source rules.
  Standalone user-provided ripen programs retain their zero-premise rules.
- Keep panic/error-checking rules; a triggered error still fails ripen.
- Observational export validates populated field types instead of rejecting an
  entire program for an unused container declaration. Set payloads use typed,
  unordered membership edges and preserve member literals/aliases.
- Save completed observational states separately as `observed-saturated-state.json`.
  These are not inserted into the certified catalog. Non-lattice merge and general
  primitive rules still require a sharing contract.
- Share repeated ground table expressions before compiling replay checks. No rows
  are materialized; updates and hidden-row check semantics remain observable.
  Primitive expressions and their enclosing calls are not assumed pure for CSE.
- Standalone ripen applies its observation limits across sweeps, returning
  `ObservedUnfinished` rather than restarting the time allowance indefinitely.

## Verification

```sh
cargo test --offline --lib
cargo test --offline --test native_fact_query --test ripen_observed
```

33 library tests passed (2 explicit audits ignored); the query suite passed 8
ordinary tests (1 benchmark ignored). The end-to-end observed-state test checks
Set export, unused Vec declarations, non-lattice merge separation, active panic
failure, and finite admission of an infinite constructor-growth program.

A diagnostic of three original `fact` history records uses the **actual native
engine**, with each application treated as a standalone coarse entry:

```sh
EGG_LAYOUT_GRAPHS=0 \
EGG_LAYOUT_AUDIT_HISTORY=/tmp/eggcc-set-final/live/history.json \
EGG_LAYOUT_AUDIT_RECORDS=888,559,1520 \
EGG_LAYOUT_AUDIT_OUTPUT=out/entry-audit \
cargo test --offline --lib recorded_entry_audit -- --ignored --nocapture
```

- Record 888 now passes preparation/replay. In a debug diagnostic, preparation
  took 0.874 s, versus 196.551 s before expression sharing/injection repair (the
  earlier run ultimately failed a pre-injection check). This is a single-case
  result, not a full analyze speedup.
- Record 559 passes preparation. A release `ripen` of its generated `entry.egg`
  completed saturation and exported 34 observed facts / 28 value vertices under
  default 50 ms / 200K admission limits. It remains **uncertified**.
- Record 1520 remains rejected: the concrete history identifies
  `AddContext(ctx,c1)` with `c1`, but the extracted boundary does not prove this.
  No union is fabricated to satisfy the output-alias check.

## Completed full replay (2026-10-04)

The same saved history was replayed with the current local release binary, two
analysis workers, graphs disabled, 50 ms / 200K facts per ripen observation, and
four steps per sweep. The first attempt aborted with a worker stack overflow at
401.924 s. Retrying with `RUST_MIN_STACK=16777216` completed without changing the
history, binary, combination population, or observation limits.

```sh
cargo build --release --offline --bin egg_layout
python3 experiments/analyze_entry_repair/replay.py \
  --history /tmp/eggcc-set-final/live/history.json \
  --output out/eggcc-full-replay-new \
  --thread-stack-bytes 16777216
```

| Full composition result | Count | Fraction of all 2,060 |
| --- | ---: | ---: |
| Prepared and registered | 594 | 28.83% |
| Saturated, observational export completed | 121 | 5.87% |
| Observation limit reached, unfinished | 36 | 1.75% |
| Local ripen error after preparation | 437 | 21.21% |
| Rejected before successful registration | 1,466 | 71.17% |
| Certified shared states | 0 | 0% |

Terminal accounting is 121 Saturated + 36 ObservedUnfinished +
1,903 Rejected = 2,060. There are no Running/WaitingEquivalent jobs left. All 121
observed-state files exist and carry `observation_only: true`; they contain 3–50
facts. Of the 594 registered compositions, 20.37% completed local saturation.
These are **composition success rates**, not held-out match/enode/eclass coverage.
The frozen fact-to-loopfact library was not rebuilt.

| Rejection reason | Count |
| --- | ---: |
| Recorded output alias not reproduced | 1,059 |
| Missing scalar literal or symbolic binding | 175 |
| Unsupported action in composition replay | 132 |
| Internal fact missing before injection | 60 |
| External fact injection failed | 40 |
| Local ripen type-check panic | 426 |
| Local ripen branch-input type-check panic | 11 |

The type-check failures concern the **extracted local context and ripen schedule**;
they do not show that the original eggcc program is invalid. The previous
1,065 unresolved-binding failures are no longer the first failing gate, but this
has exposed deeper failures rather than certifying those compositions. Registered
compositions decreased from 639 to 594 and unique validated compositions from
569 to 530; the stronger internal-row checks and changed local rule environment
mean this is not evidence of a blanket improvement in coverage.

Runtime: **423.446 s** wall time. macOS `/usr/bin/time -l` reported peak RSS
**13.088 GiB** and a separate peak memory footprint of **47.375 GiB**. They are
not interchangeable metrics. Output occupied about **1.2 GiB** despite disabled
DOT/SVG. Preparation accounted for 291.822 s (about 68.9% of total wall time);
its recorded subphases overlap and must not be summed with the total.

`full-replay.json` records the successful run, hashes, configuration, phase times,
and verification. `full-replay-attempt1.json` retains the stack-overflow attempt.
`validation.json` retains the earlier interrupted run and selected-record audit;
it is historical evidence, superseded by the complete replay for aggregate counts.
The reproducible script preserves nonzero exit status and does not label a partial
run complete. The 16 MiB stack is an explicit experiment setting, not a changed
project default or a fix for the underlying recursive execution.

## Remaining limits

Recorded equalities whose producing history is absent still need provenance;
container boundary *input reconstruction* beyond supported payloads remains
separate from exporting populated Set rows. Certified destination-state/slot
adapters and trigger-driven virtual completion are not implemented by these fixes.
Local ripen still sweeps the selected rulesets in its own order; observed results
are not a proof of equivalence to an order-sensitive source schedule.
Full analyze still retains many prepared engines/artifacts. The full run now
completes with a larger thread stack, but the memory/time cost and default-stack
overflow remain unresolved.

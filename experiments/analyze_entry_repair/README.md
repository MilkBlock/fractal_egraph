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

`validation.json` contains compact evidence. The full 2,060-composition replay
failed with StorageFull before a final report; its incomplete generated output
was removed after preserving the failure summary. **There is no new full
coverage percentage.** The frozen fact-to-loopfact library has not been updated.

## Remaining limits

Recorded equalities whose producing history is absent still need provenance;
container boundary *input reconstruction* beyond supported payloads remains
separate from exporting populated Set rows. Certified destination-state/slot
adapters and trigger-driven virtual completion are not implemented by these fixes.
Local ripen still sweeps the selected rulesets in its own order; observed results
are not a proof of equivalence to an order-sensitive source schedule.
Full analyze still retains many prepared engines/artifacts; the resource failure
must be resolved before using a full run as the next coverage baseline.

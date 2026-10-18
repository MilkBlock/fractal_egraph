# Native events, automatic Bake, target-only resume

Live transport update: the worker handshake is protocol **7**. Rebuild
`matrix.swift`; bulk live heaps now use binary frames. Offline lowered ABI **5**
and replayable JSON files are unchanged. Numeric workloads also support the
[resident fragment-table backend](../table-runtime/README.md), which sends deltas
and does not rebuild per-instance channels or solve online.

`--mode matrix-live` is an actual native egglog execution loop. It consumes the
existing analyze Cell catalog, runs native helper rules with compact producer
callbacks, drains committed row/union/rebuild events, performs boundary matching
with the shared MPS matrix program, and commits bindings through the original
native RHS sink. It does not run a second global consumer LHS query except when
`--audit true` explicitly enables the oracle.

## What persists

One Swift worker serves the entire session. Native changes create immutable
versions of source Cell frames. Programs grow by appending coordinates, conditions,
channels, templates and certified specializations. Installation verifies the old
program's immutable prefix, then copies cached feature rows, evidence, bindings
and match decisions **GPU to GPU** into the extended layout. It neither relaunches
the worker nor reads feature matrices back to the CPU to reconstruct them.

```text
native helper actions + commit/rebuild events
    → validate versioned source Cell evidence
    → shared partial binding tuples → fixed matrix conditions
    → completed binding stored on GPU
    → unknown context/alias: resumable native Bake of full parent bodies + RHS
         ├─ unfinished: keep binding and local egraph; publish no certificate
         ├─ failed: report failure; commit no original action
         └─ saturated: cache template + exact alias guard + output projection
    → hot-install extension, retaining completed matching state
    → resume targets only (no classify / GEMM)
    → submit original native RHS once
    → native events feed the next iteration
```

The native engine has one owner. No concurrent input mutation races a local Bake
poll. Between polls, input/union/subsume/delete events can retire a source frame.
Retirement cancels its uncommitted matches before resumption. Reinserted facts can
create new source versions. No old successful match can silently authorize an
RHS after its source evidence is gone.

A successful matrix binding is **not** a committed mutation. `dispatched_bindings`
counts original RHS submissions; native row/union events count committed changes.
A no-op producer callback is also not a Cell certificate: admission checks the
required visible rows and equivalences.

## Automatic specialization

The specialization key includes the frozen rule environment (session scope),
parent template/frame identities, complete native binding projection and the
exact typed alias partition. Equalities between **internal parent values** are
included, not just equality among RipenInputs. Fixed literal fields remain literal.
Only opaque equality identities are renamed; numeric parameters are not erased.

A fresh local egraph is forked from the compiled environment. Both parents' full
certified bodies are seeded, their shared identities are connected, and the
**already captured binding** is sent directly to the original RHS. Native local
saturation then derives the closure. It performs normal local rule searches;
this is Bake work, not another global search for the completed binding.

Completed recipes are reused with fresh concrete identities. Pending jobs retain
their native egraph and are polled again without restarting initialization. The
existing Ripen defaults are used as explicit observation protections: **50 ms per
poll, 200,000 facts**, checked inside native execution as well as between steps.
Rebuild may finish after the time slice to restore consistency. `--bake-ms` and
`--bake-facts` override these values. A size-limited or time-limited job is never
called saturated. `pending_matches()` and the JSON report distinguish unfinished
work from an idle completed run. A false `round()` progress flag alone is not a
completion certificate; CLI `--rounds` still bounds the number of driver polls.

The published template is a local closure summary. Its fragment projections are
registered with the existing native-source admission path and can provide future
matching interfaces once the actual global rows/equalities validate. Newly learned
instances are not left as report-only templates. The global graph commits the
original RHS, **not a bulk expansion of all locally baked facts**. Future source
admission still requires the actual global evidence. Thus this implementation
does not pretend that an abstract local certificate means every smooth fact is
already physically present globally.

## Exact reuse before matrix execution

Several Cells can provide the same typed partial query tuple. These projections
are hash-consed with their full alias obligations. This shares a matching
computation; it does **not** assert that the containing Cells have equal closures.
The live proof owner is refreshed after native changes. No representative budget,
Top-k cutoff or probability filter is used. Remaining distinct fragment parts
are combined exhaustively for the configured consumer, so their Cartesian growth
remains a limitation. Previously committed identical native bindings do not
require another Bake solely because a different Cell proof becomes visible.

## CPU work and supported domain

This mode currently uses **CPU keyed reads for source-proof admission and CPU
fragment enumeration**. This differs from the older indexed `resident` mode,
whose proof maintenance is on-device. Do not attribute that mode's zero CPU
Cell-validation counters to this one. Scalar count readbacks configure compact
MPS calls; completed/pending output bindings return to the host for native Bake
and RHS submission. Features and saved matching state stay on-device.

Live Bake currently accepts a frozen **positive constructor** ruleset and Cell
parts covering the entire consumer LHS. Numeric guards, arbitrary primitive
computations, set/merge, destructive rules, external relation parts and numeric
parameter erasure are rejected. Their existing resident/native tests remain
controls; they are not newly supported by this matrix mode. Initial Cell admission
can lag a native match until helpers establish the full source closure. Audit
reports this temporary uncovered region, including boundaries with no admitted
Cell; it is not silently reported as false or complete coverage.

The runtime retains old versions, generated programs and learned closures; no
long-running garbage-collection or speedup claim is made. Kernel recompilation,
layout growth, source admission, enumeration and local Bake still cost time.

## Reproduce

From the `codex/cell-assisted-egglog` checkout:

```sh
cargo build --offline --bin egg_layout --target-dir target/analyzer
python3 cell-assisted-egglog/validate_live.py --output out/matrix-live-check
```

The script freshly analyzes `math.egg`, builds Rust and the Metal worker, runs the
native control and live loop once each, and checks the whole constructor graph
**including equivalence**, not merely counts. `--skip-build` reuses the Rust
executable; Swift still builds. `--analyzer PATH` chooses an existing analyzer.

After that capture, the direct command is:

```sh
cargo run --offline --manifest-path cell-assisted-egglog/Cargo.toml \
  --target-dir target/cell-assisted -- \
  --mode matrix-live --analysis out/matrix-live-check/analyze \
  --ruleset '' --consumer factor --native add-commute,mul-commute \
  --parts '[{"atoms":[0],"cell":true},{"atoms":[1],"cell":true},{"atoms":[2],"cell":true}]' \
  --input cell-assisted-egglog/matrix-live/math-input.egg --rounds 6 \
  --gpu-worker out/matrix-live-check/matrix-metal --audit true
```

The tiny math rules are Add/Mul commutativity and common-factor extraction from
math-microbenchmark. Training has distinct operands; live input repeats one
operand. The verified run automatically learned **2** closures, submitted **2**
full native bindings, and produced exactly the native graph: **11 Math e-nodes /
6 e-classes**. Every channel's completion counter was at most **1**. This audited
debug run is correctness evidence, not an eggcc/pristine-egglog speedup benchmark.

`tests/live.rs` additionally checks fresh-identity recipe reuse, deliberately
unfinished Bake followed by resume, pending-task cancellation on subsume, late
union and late external facts. The service test verifies that hot installation
and target-only resume leave **both classification and match counts unchanged**;
a source retirement revokes the result. Ordinary static matrix validation is
retained. Lowered matrix ABI is version **5**; regenerate old lowered
files from their unchanged source IR.

All **53 package tests** passed with numerical workers enabled. The previous
11-case shared matrix validation also passed, including 462 native root/non-root
bindings. [Recorded runtime results](results.json) retain the compiler and worker
hashes and the exact graph comparison.

Core files: `src/live.rs` (native event driver), `src/live_bake.rs` (resumable native
closure proof), `src/matrix_service.rs` (checked hot installation), `matrix.swift`
(persistent device storage), and the existing shared IR/Metal target kernels.

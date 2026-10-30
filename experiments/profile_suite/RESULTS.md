# Cargo profile validation — 2026-10-30

The three default cases ran the **actual resident GPU backend**, with source
markers disabled and generic multi-stage Cell enabled, as selected by the user.
Each used its full frozen Pass 3 input and original finite/nested schedule.
No Python source or inference kernel was changed for this driver.

One clean measurement per case, followed by a separate encoder diagnostic:

| Case | Clean inference | Physical rows | Logical Runs | Private Runs | Diagnostic inference | Diagnostic / clean |
|---|---:|---:|---:|---:|---:|---:|
| fact | 0.324846 s | 1,089 | 407 | 36 | 1.295635 s | 3.99× |
| loopfact | 0.358976 s | 1,038 | 388 | 26 | 1.430201 s | 3.98× |
| sum-bits | 1.053600 s | 3,258 | 835 | 52 | 4.186673 s | 3.97× |

All three encoder runs exactly matched their clean reference's full state,
history, private execution counts and outer dispatch count. This is the
same-executor profiling/replay check, not an independent CPU endpoint or
extraction-quality proof. Physical rows are not Expr e-node counts.

**Small-workload profiling has substantial overhead.** Use the clean column
for speed/regression claims. Encoder distributions diagnose work but must not be
treated as a precise decomposition of clean execution. The dashboards and JSON
retain this perturbation ratio.

Cold shader setup was 257.51 / 256.60 / 256.56 seconds, respectively. It is
excluded from the inference column and explicitly retained in the results. The
default command is fast after a valid result-cache hit, not magically fast on a
cold generated Metal program. Pipeline binary archives are retained as well.

## Cache and hardware checks

- `cargo profile --cached-only`: **1.16 seconds total**, including Cargo and
  environment/fingerprint checks; 0.956 seconds inside the driver.
- All **12** compile/baseline/encoder/view entries were hits. No compiler or GPU
  inference stage launched. This is one observed invocation, not the fastest of
  repeated timing attempts.
- `cargo profile --case fact --hardware` exercised the real Apple counter path,
  completed the original program and passed the same full-state/history check.
  A second cached-only invocation reused the hardware result without recording.
  The counter template's complete recording options now participate in its key.
- Five Rust tests cover input/source identity, default selection, corrupt cache
  outputs, changed input keys, cache-only refusal, fresh-run retention,
  failed-attempt recovery and outer-command window coordinates. Clippy passes
  with warnings denied.
- The JSON tuple/list wire adapter accepts the valid compiled certificate and
  rejects an altered private schedule certificate. It does not disable checks.

Detailed numeric evidence and paths: [results.json](results.json). Large programs,
workers, traces and complete graph results remain under `out/profile-suite/`.

## Raytrace and the optional marker lane

Raytrace was **not inferred again**. Its source checksum is validated, and a
static frontend comparison found identical rules, tables, initial rows and
assertions to the retained Raytrace profile. The schedule differs only in
single-child `sequence` wrappers; removing those wrappers for comparison yields
the same control. No source/control rewrite is applied by the driver.

The optional full `--pipeline` lane generated 48 source marker tables for each
small input and passed its compilation checks. Its first `batched_cell_pack`
pipeline compilation spent over 18 minutes inside Apple's compiler before we
stopped that attempt. A CPU sample identified `llvm::FlowAnalyzer` while it was
compiling, not inference. **No marker-lane GPU performance result is claimed.**
The user's explicit choice to keep the previous resident configuration as the
default is encoded in both the CLI and the skill; the two lanes have distinct
cache keys.

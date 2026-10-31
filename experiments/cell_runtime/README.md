# Standalone Cell CPU runtime

Rust execution of shared RHS recipes, parent-slot projections and virtual fact
references, mutable function ports, deferred alias repair, exact residual queries
and finite/nested source schedules. Egglog is only a separate frontend/oracle.

Build: `cargo build --offline --release --manifest-path engine/cell-runtime/Cargo.toml`.
Check: `python3 experiments/cell_runtime/check.py --output out/cell-runtime-check`.
Shared compiler target: `python3 -m eggfuse compile --backend cell-cpu ...`.

The initial 16 native semantic regressions passed, including checkpoint/resume
and fresh-process replay. A 100-constructor RHS needs two recipe frames, but
still has per-fact metadata and indexes: this is not an RSS compression claim.

Three small Pass-3 programs execute and replay but have unresolved native
differences: extra Smaller rows; loopfact equality and sum-bits TermSingle
differences also remain. Do not exclude Smaller and declare correctness.

No GPU execution, general cross-rule closure acceleration or production Tiger
extraction is provided by this backend. Unit-cost extraction is diagnostic.

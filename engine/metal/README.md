# eggfuse Metal runtime

These are the actual Swift workers and Metal execution primitives. Compile-time
recognition and code generation live in `eggfuse/compiler/targets/metal/`.
The old backend runtime symlinks have been removed. Build and profiling scripts
read this directory directly; edit these files to change the runtime.

Build transport remains available through `experiments/device_backend/build.py`;
its provenance now includes the canonical compiler/IR source files. Python host
protocol and oracle harness migration is not complete. The canonical execution
entry is `python3 -m eggfuse run --artifact ...`; the loader validates the shared
Recipe IR and manifest before executing the precompiled program.

This relocation does not alter kernel algorithms. The full saved Pass-3 kernel
was checked byte-for-byte against the pre-migration kernel. Wider optimized Cell
coverage still requires additional verified lowering and publication contracts.


`batched_cell.metal` implements GPU-controlled preparation, multi-threadgroup
context/matrix evaluation and stable block publication. The generic worker queues
these stages without downloading bindings between logical Runs. The explicit
region pipeline uses checked derivations and per-round GPU reproducibility as
its verification contract; CPU whole-graph equality is not its acceptance gate.

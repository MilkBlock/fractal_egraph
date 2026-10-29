# eggfuse Metal runtime

These are the actual Swift workers and Metal execution primitives. Compile-time
recognition and code generation live in `eggfuse/compiler/targets/metal/`.
The old `experiments/device_backend/*.metal` and runtime `*.swift` paths are
compatibility symlinks, so saved build/profile scripts continue to resolve them.

Build transport remains available through `experiments/device_backend/build.py`;
its provenance now includes the canonical compiler/IR source files. Python host
protocol and oracle harness migration is not complete. The canonical execution
entry is `python3 -m eggfuse run --artifact ...`; the loader validates the shared
Recipe IR and manifest before executing the precompiled program.

This relocation does not alter kernel algorithms. The full saved Pass-3 kernel
was checked byte-for-byte against the pre-migration kernel. Wider optimized Cell
coverage still requires additional verified lowering and publication contracts.

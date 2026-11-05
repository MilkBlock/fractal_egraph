# Frozen eggcc Pass 3 inputs

Each case concatenates the parts listed in `suite.json` and verifies its source digest. The bundled Bake training program is a small shared training input. The Pass 3 initialization and schedule come from the named eggcc workload.

The production capture used eggcc commit [ae80f82](https://github.com/egraphs-good/eggcc/tree/ae80f82a403c1209fb084b115ee89470c7e66d0e) and the Tiger extraction pipeline. The extracted result of each pass fed the following pass. The four new inputs come from the completed capture of 96 passing workloads. Each flat file matched the recorded Pass 3 SHA-256 before splitting. Only its middle fragment is stored per case. The compiler joins it to the existing byte-identical prefix and suffix, verifies the whole-file digest and uses the same frozen Bake training input.

The sum-bits and Raytrace catalog files contain the 32 leaf blocks, bounded iterative catalog (64 total units), guarded RHS handoffs and Raytrace-trained context admission used by the fast CPU reference. Only selected-family evidence is included. A catalog has a separate checksum and is bound to its target's typed recipe. The engine has no case-name routing.

The files were extracted from the frozen guarded catalogs in the [RHS handoff study](https://github.com/MilkBlock/eggfuse/blob/4da35ee/tools/cell-compiler/benchmarks/runtime-modules/RHS_COMPLETION.md), with admission from the [context-cut study](https://github.com/MilkBlock/eggfuse/blob/70b2910/tools/cell-compiler/benchmarks/runtime-modules/CONTEXT_CUTS.md). Ordinary source compilation still performs Bake and marker selection before it can install the catalog. Other named inputs can run directly or use a separately trained catalog.

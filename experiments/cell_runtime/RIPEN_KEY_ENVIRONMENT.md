# Structural-key runtime propagation into RipSat

This fixes native Analyze/RipSat environment initialization and replay. It is
not a Cell CPU speedup, a Metal kernel optimization, or a full eggcc Pass 3 benchmark.

## Cause and repair

`source_sk.egg` declares its native structural-key runtime in a comment header.
The main capture engine installed that runtime; fresh local RipSat engines did
not. AST serialization also discarded the header, including in exported Cell
recipe JSON. A common initialization failure consequently appeared as 89
individual rejected compositions.

The repair:

- installs the declared runtime in fresh and prepared RipSat environments;
- carries it through extracted source, validation, initial source, symbolic-size
  lowering, completed replay, history, and serialized Cell recipes;
- includes it in environment identity, normalized contracts and parent-reuse
  checks; a different native definition cannot borrow the old environment;
- retains exact structural-key values as compact, shared-DAG payloads. The
  `dag-v1:` ASCII encoding survives AST string printing without JSON escape loss.
  It does not use the diagnostic hash as identity. Legacy payloads still decode;
- accepts the same payload in Metal's compiler-side constant decoder, retaining
  the existing GPU key program rather than introducing a new kernel;
- exports populated key values for observation without widening the certified
  closure contract.

## Same training input

Input: the existing sum-bits Pass 3 compiler training program, 315,811 bytes,
SHA-256 `c001ad43347ae1cb37b0e7e6d05a1c977c1b1e3cebfd09ad353aa48fc543e501`.
This is the small Bake input, not the complete sum-bits runtime graph.
The capture summary remains identical: 141 logical matches, 74 retained
applications and 67 excluded matches. Logical matches are not committed writes.

| Outcome | Before | Final repaired run |
|---|---:|---:|
| Discovered compositions | 89 | 89 |
| Undefined `CellStructuralKey` rejection | 89 | 0 |
| Prepared executable recipes | 0 | 72 |
| Reached saturation | 0 | 72 |
| Other replay-check rejections | 0 | 17 |
| Certified shareable closures | 0 | 0 |

All 72 prepared recipes retain the native runtime metadata. Their endpoints
remain **observed only**: the existing certificate exporter rejects
`unsupported container/table field sort in state export`. The 17 other
compositions fail staged fact-injection or action replay checks. Neither limit
was bypassed. Reaching saturation is not a certificate of reusable closure.

The original 50 ms / 200,000-fact limits and four-step sweeps remain unchanged.
An intermediate run reached saturation for 68 and marked four unfinished;
the final run reached saturation for all 72. This bounded observation is sensitive
to timing and is not a speedup measurement.

Counts, input/binary fingerprints and remaining reasons are in
[`ripen-key-results/sum-bits-bake.json`](ripen-key-results/sum-bits-bake.json).
Generated graphs, libraries and binaries are not tracked.

## Reproduction

From this checkout, build into an isolated target directory:

```sh
CARGO_TARGET_DIR=/tmp/eggfuse-ripen-build CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build --offline --bin egg_layout
EGG_LAYOUT_GRAPHS=0 /tmp/eggfuse-ripen-build/debug/egg_layout analyze --source /Users/mineralsteins/.codex/worktrees/d569/egg_layout/out/shared-keys-sum-bits-cpu/bake-source.egg --output out/ripen-key-recheck
```

Use a new output directory. The compiler's Analyze receipt fingerprints its
analyzer binary; rebuild/select the repaired binary before reusing the pipeline.
Old rejected analysis artifacts are not silently rewritten.

Validation: **85 library tests + 11 integration tests + 4 shared-key tests + 5 Python transport tests**
passed, with four explicitly manual tests ignored. Coverage includes cached and
fresh initialization, history replay, JSON recipe reload, symbolic-size lowering,
runtime mismatch rejection, a deep shared key DAG, DABCABC/branching reentry,
set witnesses, wildcard roundtrip, and multiple datatypes.

```sh
CARGO_TARGET_DIR=/tmp/eggfuse-ripen-build CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo test --offline --lib --test multiple_datatypes --test reentry_history_regression --test native_set_witness --test native_structural_key_merge --test wildcard_roundtrip
CARGO_TARGET_DIR=/tmp/eggfuse-ripen-build CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo test --offline -p eggfuse-structural-key
/opt/homebrew/bin/python3 -B -m unittest eggfuse.ir.test_structural_key -v
```

Do not globally disable graphs for the library suite: its renderer tests exercise
rendering. An old multiple-datatype assertion assumed relation results were Unit;
both the unmodified native binary and this build return the native `@SeenSort`.
That test now checks the native schema directly; runtime relation semantics were
not changed.

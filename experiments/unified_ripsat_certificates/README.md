# Checked RipSat certificates and runtime markers

This changes the actual native/shared CPU-GPU runtime, not a standalone cache simulation.
The structured-key transformation is separate and is not changed here.

## One pipeline, different proof methods

`Analyze evidence → checked certificate → typed marker/interface → native effect submission`

| Method | Imported evidence / checker | Runtime permission |
|---|---|---|
| Finite positive closure | Analyze's complete coarse fact forest and equality preconditions; fresh native saturation **and** independent delta replay | Materialize the proved body; skip an explicit saturation only when all active components are covered |
| Parameterized candidate selection | Source-discovered operator/function/ruleset roles; complete helper/producer universe | Fresh/incremental selection transfer and uniform-cost comparison closure under runtime obligations |
| Monotone min-plus | Typed source `min` merge and addition DAG, complete affected rule/merge dependency universe | Original synchronous numeric waves in an isolated saturation scope, with visibility, activation and overflow checks |
| Complete/candidate query | Native typed atoms, slots, guards, primitive bytecode and complete wiring | Reuse query work only; **never** authorize saturation |

`cell-assisted-egglog/src/certificate.rs` is the shared registry and report schema.
JSON is not authority: finite imports re-run the proof once at cold load;
parameterized imports revalidate current schemas, source contracts and stored wiring.
Every runtime authority binds to the current native rule-environment stamp.
Existing row identities, subscriptions and canonicalization continue to invalidate marker instances.
Ordinary finite Run boundaries are not changed into saturation.

The former `TCPair`/`Smaller`/`terms` name-based selection shortcut is now a structural
proof method (`parameterized_cell/contract.rs`). Fully renamed operators, functions,
rulesets and diagnostic text use the same checker. The six minimum-selection axioms
are a sufficient proof schema, not a claim to recognize arbitrary equivalent programs.
Legacy `bake_terms`, `new_joint`, and fixture schedule constants remain compatibility APIs;
default Analyze export uses the generic version-2 certificate format.

## Marker compilers

- Symbolic `tools.cell_spine.compiler` first asks the native certificate checker to
  reprove the Analyze seed forests. Its old family sharing is checked against the
  fresh closure graphs. Distinguished roots/port orders retain separate interfaces.
  The existing marker layout solver then chooses anchor, payload and publisher layouts.
- Scheduled compilation accepts `--certificate-library FILE` or an Analyze directory
  containing `certificates.egg`. Already certified queries are reserved for the loaded
  runtime; the solver handles the remaining source queries. It writes the checked
  certificate ledger alongside `boundary-programs.json`. The source stays unchanged.
- The event-loop observer accepts `CELL_CERTIFICATE_LIBRARY`; the bench skill exposes
  `run/audit/spine-program --certificate-library FILE` and fingerprints the file.
  Stored plans coexist with additional boundary-query layouts. Changed coverage,
  native wiring, guards or unproved closure behavior is rejected.

The five-state If/Single catalog is not a proof of the entire eggcc Pass 3. Likewise,
loop-context fixed wiring is a query certificate, not newly proved whole-Cell saturation.
No new performance benefit is inferred merely from a shared report format.

## Reproduce

From the repository/worktree root, build once:

```sh
cargo build --offline --release --target-dir target/analyzer --bin egg_layout
```

Automatic finite import; no hand-authored transition plan:

```sh
mkdir -p out/certificate-example
target/analyzer/release/egg_layout bake-certificates \
  experiments/analyze_library_credit/library out/certificate-example/finite.egg
target/analyzer/release/egg_layout run-certificates out/certificate-example/finite.egg \
  experiments/unified_ripsat_certificates/finite-heldout.egg out/certificate-example/finite-run
```

Analyze committed history and export a numeric certificate without any `terms` symbols:

```sh
target/analyzer/release/egg_layout analyze-transfer \
  experiments/unified_ripsat_certificates/scalar-training.egg out/certificate-example/scalar.egg
target/analyzer/release/egg_layout run-certificates out/certificate-example/scalar.egg \
  experiments/unified_ripsat_certificates/scalar-heldout.egg out/certificate-example/scalar-run
```

Full Analyze also accepts `--bake-certificates` (legacy alias `--bake-transfers`).
Unsupported proof methods retain native execution and record why no certificate was
exported. This is not a universal closure decision procedure. Finite automatic import
currently admits one positive constructor/equality ruleset; parameterized schemes
cover the explicitly checked source contracts above.

For symbolic compiler validation with this build directory, set
`CELL_CERTIFICATE_CHECKER=$PWD/target/analyzer/release/egg_layout`.
The default checker remains `target/release/egg_layout`, matching that compiler's build.

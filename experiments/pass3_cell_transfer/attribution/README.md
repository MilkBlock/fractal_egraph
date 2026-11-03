# Measuring the value of a baked Cell

A Cell is useful on a target when its verified execution avoids more work than it adds on that target. Count the work that would have occurred at the same original Run under the actual baseline. A successful handoff can legitimately save zero candidate visits, zero key lookups and zero constructors.

## Three different dependencies

1. The historical [certificate-based compiler](https://github.com/MilkBlock/eggfuse/blob/bf5b4539d09ad96473e40875868a8f4f02d6add5/tools/cell_spine/compiler.py) could substitute positive constructor/equality templates from a Cell catalog.
2. The current [Rust marker pass](../../../tools/cell-compiler/src/markers.rs) flattens the target rule bodies, discovers alpha-equivalent positive constructor fragments, selects a nonoverlapping cover and interns structural prefixes. It emits ordinary Unit-valued tables, producer RHS publications, exact completion rules, rewritten consumers and source schedules. For example, sum-bits has a `Const(Int(k), ty, context)` view used at 15 consumer positions with zero Bake witnesses even under the artificial training fixture. The native frontend lowers these generated rules into the artifact's typed core. The runtime also checks the entry and continuation plans against this core. An enabled persistent Spine captures facts from the *current* execution after commit and repair.
3. A transferred two-step executable additionally requires an admitted Bake class with a real producer/consumer handoff, a compatible target rule pair and valid runtime guards.

We compiled the identical sum-bits source with the usual artificial fixture and again using the shared declarations and rules followed by `(run-schedule (repeat 0 (run terms)))`. The second Bake completed with zero registered Cells. Both compilations selected 30 fragments, emitted 34 views, 107 receipt tables, 241 shared entry nodes and 256 stage entries. Three views had training witnesses with the artificial fixture and none did with the empty fixture. The marked source was byte-identical. The only varying program fields were `marker_program_sha256` and its enclosing `sha256`. The filtered executions agreed on final state, work counters and source scheduling fields. Turning on the persistent Spine in both cases yielded 3,034 resumes and 867 first visits by a context to another ruleset, with identical endpoint graphs. These numbers describe this artifact and configuration.

## Controlled training comparison

Both training sources use the same deployed sum-bits typed-rule ABI, class admission policy, native collector and cap of eight shapes and eight observations per shape. Target workloads never select classes. An additional admission-only diagnostic uses each training program's own unmarked ABI to locate losses at the compatibility check.

| Training source | Native records | Physical cross-ruleset handoffs | Eligible before sampling | Two-step classes emitted |
|---|---:|---:|---:|---:|
| Artificial terms fixture | 74 | 101 | 0 | 0 |
| Original sum-bits | 4,444 | 3,055 | 9 | 7 |

All 101 artificial handoffs failed exact typed query/effect compatibility with the marked deployment. Even granting the artificial source its own ABI, 89 had no checked functional edge and 12 had an anchor alone with no remaining operation or reusable construction. The artificial portfolio therefore links to the same executable program as the empty portfolio on all eight workloads. Its ordinary analyzer's 72 registered Cells and 56 classes count a different population.

Of the 3,055 real handoffs, 1,119 hit the initialization exclusion, 998 had different typed queries/effects, 424 failed other typed-rule equality checks, 504 had no checked functional edge, one was anchor-only and nine qualified. Under the real source's own unmarked ABI, 40 observations and 12 shapes qualified before sampling. The deployed ABI admitted four shapes. The nine admitted pairs gave 18 prepared units, 14 classes and seven two-step executable classes. The other seven classes are single producer preparations. They have no standalone cross-stage deployment under this policy. The class ledger records them explicitly. [Training ledger and marker negative control](evidence.json) · [Per-class measurements](RESULTS.md)

## Contribution model

For a class `i`, a workload `w`, and a set `S` of *other* enabled classes, define its marginal time saving as `T(w,S) − T(w,S+i)`. Use the same subtraction independently for row visits, key lookups, constructor requests, and registered Cells. A positive saving is good. These counters have different units and must not be added with arbitrary weights.

- **Validity and admission.** Record the source, observed dependency, representative, guards, residual code and any rejection. Require the current ports, source schedule, original RHS and visibility/epoch semantics to hold. The frozen units here are verified finite handoffs. Their isolated training endpoints do not confer a general terminal-state substitution certificate.
- **Exposure and yield.** Record root cursors opened, candidate dispatches to the compiled edge, literal misses, post-guard attempts, port rejections and actual cross-Run RHS applications. For this backend, dispatch opportunities equal literal misses plus attempts. `applications / opportunities` and `applications / roots` answer different questions. A zero denominator has undefined yield. A failed class literal guard falls back to ordinary matching. A checked missing port may instead refute a branch and save a scan even when no RHS applies; compare those rejected paths against their baseline cost separately.
- **Work displaced.** Measure the counterfactual row visits, key lookups, constructions and storage registrations. Count known-atom and constructor-reuse instructions actually taken and remaining ports read. Report savings per application only alongside total exposure. A class firing 100 times can be less useful than one firing twice if the 100 applications repeat baseline point reads.
- **Shared costs.** Singletons measure `T(empty) − T(i)`. Leave-one-out measures `T(library−i) − T(library)`. Adding singleton deltas need not reproduce the library result. Classes can share a dispatch edge, producer cursor and storage policy. For an additive attribution of deterministic work, average each class's marginal work over every insertion order (Shapley attribution). The sum then equals the measured library-minus-empty difference, with the appropriate sign. We use work counters for this exact calculation.
- **Deployment return.** Weight held-out workload savings by the expected number of executions. Subtract incremental training, linking and startup costs over the same period; measure memory separately against a budget. Break-even needs positive recurring savings. Record compilation and byte costs separately because several classes can share a training pass. A small timing difference, especially with zero eliminated work and few paired samples, is weak evidence for admission.

For a deployment with workload probabilities π and N invocations, the measured time objective is `N × Σ_w π(w) × [T(w,S) − T(w,S+i)] − incremental Bake/link/startup time`. Work on accepted paths and proven-empty paths can both pay for guard checks, failed dispatches and maintenance. A measured counter is a mechanism for that gain, not a universal conversion from visits to milliseconds. Keep the training and validation workloads separate when choosing π or admission thresholds.

On sum-bits, class 9 adds 158 registered Cells on its own. Removing it from the full library releases zero registrations because other classes still arm the same rule paths. Its exact share averaged over all insertion orders is 31.6 additional registered Cells. All 128 subsets have the same candidate visit and key lookup counts. This makes the choice of reference portfolio material even for integer-valued work.

The resulting selection criterion is testable: first find expensive residual work in the target baseline, then find a portable guarded class that can remove it, validate the proof and measure the *marginal* improvement on held-out inputs. Here the seven residual programs contain only `query` instructions and no constructor-reuse instruction. Class 3 checks two tables and two primitives; each other class still checks one table. None carries a checked `known` residual instruction. Frequency ranking alone cannot distinguish them by saved work.

## Reproduce

From the repository root, after compiling sum-bits into `out/unit-training`:

```sh
unit() { cargo run --release --locked --manifest-path tools/cell-compiler/Cargo.toml --example baked_unit -- "$@"; }
EGG_LAYOUT_GRAPHS=0 unit catalog out/unit-training/source.egg out/unit-training/program.json out/real-catalog
EGG_LAYOUT_GRAPHS=0 unit catalog out/unit-training/bake-source.egg out/unit-training/program.json out/artificial-catalog
unit measure isolated out/real-catalog/catalog.json out/unit-training/program.json 7 out/singletons.json
unit measure marginal out/real-catalog/catalog.json out/unit-training/program.json 5 out/marginals.json
unit shapley-work out/real-catalog/catalog.json out/unit-training/program.json out/shapley.json
```

Use new output paths. `joint` compares empty and full libraries. `audit-library` takes exactly one work-only observation per coalition and drops timing fields. Portfolio linking checks each relocated instruction sequence and groups alternate guards on the same edge by disjunction. A match executes the consumer once. Blocked timings exclude load, setup, verification and export. Endpoint hashes and per-Run schedule/binding-count signatures must agree in every lane. These checks supplement the detailed stage audits in the [original experiment](../README.md).

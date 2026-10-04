# Parent RipSat snapshot grafting: differential native experiment

**Parent-snapshot continuation agrees with ordered reconstruction in the tested
finite cases and saves repeated parent computation. This is a standalone harness
using the actual local egglog runtime, not a change to default `analyze`, not a
pristine-egglog comparison, and not terminal-state caching.**

## Three paths, one explicit contract

1. `rebuild`: clone the compiled environment, inject the parent seed, saturate the
   parent, inject the graft, then saturate again.
2. `snapshot`: bake that parent once, `fork_shared()` its native state for each
   graft, inject the same graft, and continue native incremental execution.
3. `fused`: inject seed and graft before any saturation. This intentionally
   changes history; it is a diagnostic counterfactual, not the correctness oracle.

All paths retain source rules, bindings, literal values, union and visibility.
The comparator checks every completed step's full typed state, including aliasing,
Set membership and subsumed rows. Errors must agree in phase, message and partial
state. Every snapshot fork is also checked against the unchanged parent, including
an invalid branch followed by a valid sibling. Two successive grafts exercise
continuation beyond the first child state. Time-limited states are inconclusive,
never equivalent. Different workloads, case counts and limits cannot be compared.

The parent supplies the compiled environment in snapshot mode; the unused scratch
engine is released. Keeping an additional scratch engine would increase memory.
No cross-parent slot remapping or replacement of a child by a cached terminal state
is performed. Graft code names concrete boundary constructor terms inside the
fork, so it does not transfer newly allocated values between independent engines.

## Workload and measurement

- Ten self-contained contract grafts cover external reads, union-enabled joins,
  sequential transitions, duplicate effects, `set :merge new`, subsume, errors,
  recovery on another fork, and unbounded constructor growth.
- Five real grafts are **all distinct `Get` operand terms** from the initialization
  of the actual eggcc `fact` export. Each is unioned with the single Expr slot of
  prepared getter cell 1706 from the earlier saved analysis. The parent has 11
  rows and 12 logical matches during its bake. The original eggcc optimizer
  schedule is retained; initialization is only used to enumerate the input terms.
- Three paired repetitions, alternating process order, 50 ms per closure and
  200K facts. Every mode runs in a separate process; `/usr/bin/time -l` measures
  the full process's peak RSS and footprint.
- Execution timings exclude setup compilation, serialization and comparison.
  Setup and full-process wall time are reported separately. Amortized snapshot
  time includes its one-time parent bake divided over that batch's grafts.
- Row-sharing counters count only frozen backing allocations; they exclude owned
  rows, indexes and metadata. They are not overall memory compression ratios.

## Correctness

In every repetition, ordered rebuild versus snapshot yields **13 equal, zero
different, two inconclusive** cases: 12 successful closures and one identical
error outcome are included in the 13 equal cases. The real `If` graft and the synthetic growth
case hit limits. Four other real eggcc grafts reach identical final states:

| Real graft | Rows | Typed value vertices (not just e-classes) |
| --- | ---: | ---: |
| Get input 0 | 121 | 86 |
| Get input 1 | 121 | 86 |
| Get input 2 | 202 | 148 |
| Get input 3 | 143 | 103 |

The fused path differs on **mutable-table** and **visibility** cases. For example,
`Seen(x,1)` produced before replacing `score(x)` survives ordered execution;
replacement before running the parent never produces it. Likewise, subsuming A
before A→B fires differs from subsuming it after B exists. Parent saturation and
graft injection cannot be reordered merely because their final inputs look alike.

## Time and memory on equal finite states

To avoid comparing memory for different-sized unfinished graphs, a second batch
retains only cases equal in all three prior comparisons. Both modes receive that
same frozen subset: **13 equal / 0 different / 0 inconclusive** each repetition.
The selection report hash is recorded and its workload must match exactly.

For the four completed **real eggcc** grafts, the mean of per-case timing medians:

| Execution | Milliseconds per graft |
| --- | ---: |
| Ordered rebuild | 33.10 |
| Snapshot continuation, parent already baked | 16.97 |
| Snapshot including amortized parent bake | 21.26 |

That is about **49% less incremental time**, or **36% less with bake amortized**.
Each child performs the same logical matches (187, 187, 305, 225 respectively).
The four cold parents perform 48 logical matches; the shared parent performs 12
once. This saves repeated parent preparation/saturation, not the child's boundary
Join work. These are logical matches, not committed mutations.

Across the entire matched finite batch (contracts plus eggcc), process medians:

| Mode | Wall ms | Peak RSS MiB | Peak footprint MiB |
| --- | ---: | ---: | ---: |
| Ordered rebuild | 276.36 | 98.23 | 80.06 |
| Snapshot | 230.76 | 96.19 | 78.74 |

RSS samples were 98.20/101.20/98.23 MiB versus 96.19/96.80/91.94 MiB.
**This is not strong evidence of memory compression.** The actual parent row
backing is only 256 bytes; before a graft, two logical copies share those 256
bytes. Most process memory is elsewhere. Three repetitions and one real parent
family do not establish general speed or memory improvements.

## Reproduce

```sh
cargo test --offline --example ripsat_graft
cargo build --release --offline --example ripsat_graft
python3 experiments/ripsat_graft/run.py \
  --analysis /tmp/eggcc-set-final/live --cell 1706 \
  --output out/graft-full
python3 experiments/ripsat_graft/run.py \
  --analysis /tmp/eggcc-set-final/live --cell 1706 \
  --output out/graft-finite --finite-from out/graft-full/summary.json
```

The real-source experiment requires the saved prepared analysis from the earlier
cell probe, including its initialization rule. `--cell` selects that prepared
one-Expr-slot parent; IDs are not assumed portable between unrelated captures.
The small contracts can run without that external artifact:

```sh
target/release/examples/ripsat_graft run \
  experiments/ripsat_graft/contracts.json snapshot /tmp/graft-contracts.json 50
```

`results.json` preserves compact results and hashes. Generated workload, per-mode
snapshots and paired comparisons live under the output directory. No default
planner, automatic composition discovery, or certified sharing catalog was changed.
The next integration would need to supply a verified parent/slot mapping and the
exact ordered external effect delta; these experiments do not infer that mapping.

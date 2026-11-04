# Ablating local congruence, union graft and stage Spine

The control is the standalone single-threaded Cell CPU executor at `b820a0b` with the planned Free Join lane, shared entry scheduling, Sieve, column intersection, ordinary storage and default merge programs. Each timed lane for a workload uses the same release binary and frozen compiler artifact. The engine settings apart from the requested modules are identical. Source compilation, artifact loading, initialization, graph export and checksums are outside the inference timer.

## Two frozen eggcc Pass 3 schedules

| Requested modules | sum-bits median, ms | Raytrace median, s | Raytrace vs control |
|---|---:|---:|---:|
| All off | 74.82 | 31.98 | +0.0% |
| Union graft | 94.91 | 36.92 | +15.4% |
| Stage Spine | 110.08 | 53.72 | +68.0% |
| Union graft + stage Spine | 122.78 | 57.21 | +78.9% |

One warm-up per effective configuration was excluded. Run order rotated each round, with fresh processes and graphs, seven measured rounds on sum-bits and three on Raytrace. The conventional candidate count covers direct and residual rows, merge candidates, Spine pivot candidates and shared-node predicate checks. The next column includes the physical candidate pairs visited inside the graft matcher. Neighborhood reads during publication are separate maintenance work. Entry probes count index probes before seed delivery. These operator counts can revisit the same row.

| Workload | Modules | Historical candidates | Candidates incl. graft pairs | Neighborhood rows read | Physical entry probes |
|---|---|---:|---:|---:|---:|
| sum-bits | All off | 15,549 | 15,549 | 0 | 16,537 |
| sum-bits | Union graft | 14,323 | 15,954 | 5,703 | 16,537 |
| sum-bits | Stage Spine | 16,532 | 16,532 | 0 | 0 |
| sum-bits | Union graft + stage Spine | 15,092 | 16,831 | 5,703 | 0 |
| raytrace | All off | 9,522,225 | 9,522,225 | 0 | 3,155,529 |
| raytrace | Union graft | 9,388,999 | 9,579,138 | 1,026,552 | 3,155,529 |
| raytrace | Stage Spine | 9,828,851 | 9,828,851 | 0 | 0 |
| raytrace | Union graft + stage Spine | 9,662,557 | 9,873,192 | 1,026,552 | 0 |

## Local congruence

Both compiler artifacts contain 83 templates with local proofs. The sum-bits artifact has 493 conclusions and Raytrace has 12,118. Both have `reorder_safe=false`. The current procedure checks the merge policy of every non-virtual table before allowing constructor unions to run ahead of global repair. Both artifacts have 148 ordered and eight unique contracts outside the allowed set. A run explicitly requesting `CELL_CPU_CONGRUENCE=1` on each input confirmed the effective switch was off, with zero guard tests, proved unions and coalesced facts. On these two artifacts the eight nominal combinations collapse to the four effective configurations in the table.

The positive control is [a small constructor-only source](congruence-chain.egg) processed by the same Rust compiler and runtime. Eight independent pairs each acquire two four-deep constructor chains. A later Run unions each pair of leaves. With local congruence enabled the runtime tested 16 guards, installed 32 proved unions and coalesced 32 facts. With the switch off it installed zero local unions. All eight switch combinations agree on the final state and per-Run schedule and binding counts. The control exercises three short Runs. Its off/on medians were 0.265/0.315 ms across seven samples. These timings apply only to this small fixture.

## Union graft

The graft matcher examines complete neighborhoods published after effective unions for eligible two-constructor joins. Candidate pairs go through the normal row and delta-owner checks. The historical count omits this loop. On Raytrace graft alone reduces that count by 133,226 but checks 190,139 pairs. With pairs included it adds 56,913 row or pair visits, plus 1,026,552 neighborhood row reads for 160,959 publications. On sum-bits it saves 1,226 historical visits, checks 1,631 pairs and reads 5,703 neighborhood rows. A routed seed can produce multiple bindings.

Graft also enables union lineage and disables the lazy constructor-only RHS fast path. In a separate sum-bits control `CELL_CPU_UNION_TRACE=1` alone already recorded 1,039 lineage events and reduced avoided RHS frames from 3,079 to zero. Graft incurs neighborhood construction and routing on top of this. In that separate seven-sample comparison, medians were 74.26 ms with all modules off, 76.63 ms with lineage alone, and 91.18 ms with graft and its required lineage.

## Stage Spine

The primary matrix tests the literal `CELL_CPU_STAGE_SPINE=1` switch. The partial, receipt and private shared-entry suboptions stay at their defaults. The ordinary source scheduler and global shared-node schedule are active. The runtime disables the early Sieve and Free Join entry branch globally when Spine is on, including for rules where Spine never takes over. Later filters and residual Free Join can still run. Spine also disables the lazy constructor-only RHS path.

| Stage Spine work | sum-bits | Raytrace |
|---|---:|---:|
| Resumed steps | 3,034 | 663,403 |
| Resumptions entering a ruleset new to this Spine | 867 | 134,299 |
| Maximum chain | 12 | 29 |
| Seeds delivered, control → Spine | 10,326 → 58,905 | 3,437,773 → 33,476,641 |
| Capture lookups, Spine → Spine with receipts | 16,425 → 58 | 6,002,351 → 9,365 |
| Direct receipt hits | 16,367 | 5,992,986 |

Receipts reuse existing witnesses to acquire facts for a continuation. A separate diagnostic run on each artifact reproduced the same delivered seeds, work on covered entries and final state while reducing capture lookups. The residual-only Free Join diagnostic keeps the early Sieve active and switches off the planned entry solver. It delivers 32,792 seeds on sum-bits, compared with 58,905 under Spine. These are distinct changes to where work happens; the standalone switch experiment measures their combined effect.

A separate seven-pair sum-bits control equalized three execution-path differences. Both sides used residual-only Free Join, disabled Sieve and enabled union trace, which suppresses the lazy constructor frame shortcut. Both delivered 58,905 seeds. With Spine off / on, historical candidate visits were 16,735 / 16,735 and inference medians were 93.58 / 115.35 ms. This isolates the marginal effect of the existing Spine implementation under that specific matcher.

## Phase costs on Raytrace

| Modules | Search, s | RHS, s | Repair, s | Query update and publication, s |
|---|---:|---:|---:|---:|
| All off | 15.57 | 8.99 | 1.88 | 4.54 |
| Union graft | 16.00 | 9.29 | 2.40 | 8.19 |
| Stage Spine | 23.08 | 13.28 | 1.88 | 14.49 |
| Union graft + stage Spine | 22.92 | 13.26 | 2.33 | 17.70 |

The query update timer includes publishing Spine contexts and graft neighborhoods as well as updating indexes. Search, RHS and maintenance therefore all matter when deciding whether to enable these modules. On these two frozen schedules the all-off configuration gives the lowest inference median.

## Correctness and reproduction

The small real workload matched at its initial graph and every one of the 1,428 Run snapshots, including ordered RHS input values and schedule fields. The combined audit also requested local congruence to exercise its safety gate. On Raytrace, every measured configuration matched the byte-for-byte full graph endpoint, binding total and schedule digest over all 16,883 Runs. That digest covers ruleset, path, iterations, epoch, logical and executed counts and the changed flag. Intermediate Raytrace graphs and individual RHS input values were outside this audit. These are Cell-to-Cell comparisons.

```sh
cargo cell-compile --case sum-bits --emit-free-join-entry --output out/modules-sum
cargo cell-compile --case raytrace --emit-free-join-entry --output out/modules-ray

for program in out/modules-sum/program.json out/modules-ray/program.json; do
  cargo cell-bench --program "$program" --lane free-join-planned --repeat 3
  for modules in local-congruence union-graft stage-spine union-graft,stage-spine; do
    cargo cell-bench --program "$program" --lane free-join-planned --modules "$modules" --repeat 3
  done
done

cargo cell-bench --program out/modules-sum/program.json --lane free-join-planned \
  --modules local-congruence,union-graft,stage-spine --repeat 1 --verify-stages

cargo cell-bench --source tools/cell-compiler/benchmarks/runtime-modules/congruence-chain.egg \
  --lane free-join-planned --modules local-congruence --repeat 5 --verify-stages
```

With `--modules`, stage verification compares the requested set against the same lane with those modules off. The worker clears ambient `CELL_CPU_*` settings. Its JSON distinguishes requests from effective activation and reports graft pairs, neighborhood reads, resumed steps, chain depth and local proofs. For comparable fresh-process timings, use `--repeat 1` for each configuration in each rotated round. [Raw timing samples, phase medians, counters and state digests](results.json)

The follow-up routes only live, certified rule entries to Spine and restores early filtering on the other entries. [Routed Spine comparison](ROUTING.md)

The producer slot transfer experiment measures physical Row decodes, binding copies and parent-backed RHS frames. [Slot transfer ablation](SLOT_TRANSFER.md)

The [Spine combination study](SPINE_SHAPES.md) measures applied cursor lineages and transfers precompiled consumer blocks between frozen Pass 3 programs.

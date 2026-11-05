# Computed equality keys and narrower producer publication

The input is eggcc Pass 2 of 3mm and 2mm, with the transferred sum-bits catalog used in the CPU tech report. The ordinary control uses single-threaded release, planned Free Join, shared entries, Sieve, column intersection, static aliases, range guards, Stage Spine and receipts. The delta-neighbor experiment is off. The added indexes and publication restriction are opt-in. [Raw samples and phase audits](computed-key-results.json)

## Two directions of an arithmetic join

Rule 100 joins two slices of the same expression and requires the right start to equal the left start plus length. A later atom checks the concatenation of their results. The ordinary right pivot expands by expression and then checks the sum. The left pivot computes the sum, but a subtrie made during seed admission may still cover only the expression. The new compiler derives both refinements from typed slots, the expanding atom and the primitive. It records the plans separately from the existing kernel encoding and checks them when loading an artifact. Legacy artifacts derive the same proof.

For the reverse pivot the index key is (bound prefix, checked sum of two row columns). For the forward pivot the second equality uses a row column and the result of an earlier sum. Both use the existing versioned range-index infrastructure. Updates after repair remove old memberships and enter the new canonical key. A checked sum that overflows has no entry. On domains of at least eight witnesses, probe the exact bucket and intersect it with the existing ordered witness list. The original predicate, delta owner, visibility, sort order and complete RHS input checks still apply. Neighbor-cache expansions keep their own full-domain cache contract. A new Bake round cannot certify the absence of dynamic siblings at this partial-key expansion; the checked index can.

| Input | Left pivot visits | Right pivot visits | Concat pivot visits | Rule search, ms | Index build + update, ms |
|---|---:|---:|---:|---:|---:|
| 3mm | 626,000 → 43,372 | 654,494 → 41,560 | 5,433 | 164.4 → 72.3 | 18.8 |
| 2mm | 451,886 → 33,699 | 473,806 → 31,810 | 4,487 | 117.6 → 54.4 | 14.9 |

Times in this table are diagnostic samples. The 3mm pivots produce 12,705, 19,144 and 5,957 pre-dedup answers in both arms; 2mm produces 9,549, 14,520 and 4,737. The reverse-only pilot did not reduce the left pivot's visits. The reported candidate count excludes index construction, updates and binary membership tests; those costs remain inside inference.

## Spine captures

The producer allowlist is the set of tables consumed by validated leaf blocks. Each producer still captures every eligible output to those tables in original order, preserving same-key last-writer handling. Ordinary matching covers unclaimed entries. Catalog scope deliberately gives up some working Spine resumptions. The real 3mm program admits 1,830 of 3,103 eligible producer operations. Publications fall from 613,253 to 391,260 and resumptions from 158,525 to 131,590. For 2mm it admits 1,556 of 2,677 operations; publications fall from 491,302 to 316,991 and resumptions from 127,848 to 106,142. Maximum chain lengths remain 28 and 26. The baseline, key-only, catalog-only and combined arms match the ordered RHS digest at each of 3,905 and 3,658 Runs, the schedule digest and the full final graph. The winning cursor tree matches the baseline for key-only and changes for catalog scope.

| Input | Control | Computed keys | Keys + catalog publication | Same options, rejected marker bundle |
|---|---:|---:|---:|---:|
| 3mm | 3,522.9 | 3,490.8 | 3,285.8 | 3,205.0 |
| 2mm | 3,289.0 | 2,637.0 | 2,551.4 | 2,505.3 |

Inference medians in milliseconds, one excluded warm-up and four measured rounds. The fourth column of numbers uses different compiled input. The separate rotated comparison below includes fresh unmarked native egglog (report tag) and gives inference / full CLI medians in milliseconds. The Cell CLI exports its final graph and report; native CLI output obligations differ.

| Input | Native | Cell control | Cell keys + catalog |
|---|---:|---:|---:|
| 3mm | 2,584.2 / 2,676.0 | 3,813.6 / 5,636.0 | 3,481.2 / 5,098.3 |
| 2mm | 2,068.3 / 2,148.5 | 2,856.1 / 3,915.5 | 2,660.9 / 4,469.9 |

## Marker admission

A paired native profile binds both exact input digests to three or more fresh-process pairs on the same native timing adapter. After one excluded warm-up pair, alternate order and compute the median of (marked minus pre-marker) search/apply + merge + rebuild per pair. A negative delta admits the selected bundle; a tie or loss emits the pre-marker structural-key source and clears marker and receipt metadata. The proposal and the decision are recorded in marker-selection.json. Measured after structural-key rewriting; compilation, graph export and extraction are outside these costs. V2 fragment profiles can instead supply separately measured marginal rebuild costs. These two experiments measured the bundle and did not estimate individual marker costs.

| Input | Paired native rebuild delta | Paired native total phase delta | Decision |
|---|---:|---:|---|
| 3mm | +224.1 ms | +155.5 ms | Reject bundle |
| 2mm | +185.3 ms | +136.2 ms | Reject bundle |

This comparison changes the source program. On the original table names, the marked versus rejected Cell endpoints contain 240,029 versus 239,213 rows on 3mm and 191,798 versus 191,151 on 2mm. A separate dump of the pinned native engine's tables on marked versus pre-marker source also differs, with 239,557 versus 239,278 rows and 191,460 versus 191,168 rows respectively. Differences include Smaller, TCPair, TermGet, TermSingle and TermConcat. Neither full cross-source graph equivalence nor extraction equivalence was established. The timing for the rejected bundle is recorded as a separate deployment experiment; it is excluded from the matched-source speedup.

## Reproduction

Use the 64-unit donor and the captured Pass 2 artifacts from the CPU report (59 units survive transfer here). To reproduce the paired marker calibration, build the report's counter-disabled native count_fj adapter, then use fresh output paths.

```sh
cargo cell-calibrate-markers --compiled "$compiled" --native "$native_time" --output costs.json --pairs 3
cargo cell-compile --source "$compiled/source.egg" --bake-source "$compiled/bake-source.egg" \
  --emit-free-join-entry --native-marker-profile costs.json --output rejected-or-admitted
export CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1 CELL_CPU_SHARED_NODE_SCHEDULE=1
export CELL_CPU_FREE_JOIN=planned CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1
export CELL_CPU_KNOWN_RESULTS=static CELL_CPU_RANGE_GUARDS=1 CELL_CPU_SLOT_TRANSFER=off
export CELL_CPU_FD_KEYS=0 CELL_CPU_SCALAR_MERGE=0
CELL_CPU_COMPUTED_KEYS=1 CELL_CPU_SPINE_PUBLICATION=catalog cargo run --release --locked \
  --manifest-path engine/cell-runtime/Cargo.toml --features iterative-bake -- \
  --program "$fixed" --output scoped-run
```

Omit both new environment settings for the control. Enable only computed keys to isolate the index. Set CELL_CPU_PROFILE=1 in a separate process for per-rule/per-pivot residual visits and produced bindings. Use iterative-bake,spine-shapes,slot-transfer-audit for ordered RHS and winning-tree verification.

# Cost of sibling slot transfer and parent-backed RHS frames

The frozen sum-bits and Raytrace Pass 3 programs run on the standalone Cell CPU. All lanes use planned Free Join, shared entries, Sieve, column intersection, first-resumption Spine and receipts, static constructor aliases, range guards and ordinary storage. The compiler proof, schedule, workload and release executable are held fixed. The default is `off`. The previous `--modules slot-transfer` and `CELL_CPU_SLOT_TRANSFER=1` select `both`.

| Mode | Consumer execution |
|---|---|
| `off` | Original row decoding and RHS frames. |
| `siblings` | Read compiler-proven sibling slots in the ordinary matcher or a scheduled multi-atom Spine consumer. |
| `parent` | Check single-table Spine producer/route eligibility and project consumer frame fields through the retained parent fact. |
| `both` | Enable both paths and share the producer/route check when both paths apply. |
| `local-only` | Enable sibling transfer only in the ordinary matcher. This isolates the additional Spine sibling route search when both modes decode the same number of Rows. |
| `parent-check` | Perform the single-table proof checks and count accepted answers, then apply with ordinary owned inputs. |
| `parent-owned` | Attach and retain the same parents as `parent`, and keep projected slots in the existing owned slot map. The global diagnostic frame policy also applies to ordinary parents. Both workloads had zero ordinary parent frames in the control. |

A separate diagnostic build counts physical Rows, copies and work at the proof and frame boundaries. A *producer check* is an eligible Spine seed subjected to the owner check, including witnesses rejected before table lookup. A *route lookup* is an actual multi-stage route map probe after that check. A *parent read* is a traversal from a consumer frame slot into a retained parent column, including recursive traversals. Reads are classified by whether the engine is executing a rule RHS. Stored parent frames count successful physical registrations.

## Matched work and correctness

| Raytrace mode | Rows decoded | Spine owner checks | Route lookups | Parent-backed stored frames | Parent reads during RHS | Parent reads elsewhere |
|---|---:|---:|---:|---:|---:|---:|
| off | 16,006,459 | 0 | 0 | 0 | 0 | 0 |
| siblings | 15,847,710 | 1,657,700 | 287,998 | 0 | 0 | 0 |
| parent | 16,006,459 | 317,796 | 192,862 | 153,669 | 748,916 | 1,115,717 |
| both | 15,847,710 | 1,872,925 | 449,932 | 153,669 | 748,916 | 1,118,800 |
| local-only | 15,847,710 | 0 | 0 | 0 | 0 | 0 |
| parent-check | 16,006,459 | 317,796 | 192,862 | 0 | 0 | 0 |
| parent-owned | 16,006,459 | 317,796 | 192,862 | 153,669 | 0 | 0 |

Both sibling modes remove 158,749 complete graph Row decodes on Raytrace and 523 on sum-bits. Their multi-atom Spine read count is zero on both programs. All those savings occur in ordinary continuation matching. Compared with local-only, enabling sibling routing in Spine performs 1,657,700 owner checks and 287,998 route lookups on Raytrace, including 744 more attempts to wire an effect. The single-table parent mode independently removes 471,458 owned RHS frame fields on Raytrace and 3,315 on sum-bits. Both sibling modes avoid 576,015 fresh binding/seal copies on Raytrace and 1,698 on sum-bits.

All seven lanes were audited on each workload. Before applying each Run, the diagnostic build hashes the ordered sequence of rule IDs and slot/value maps. The entire list of digests compared byte-for-byte to the previous frozen control and also matched its published SHA-256. Each timed and untimed sample matched the frozen final state digest, per-Run schedule digest, effect totals, construction requests, physical Cell count, candidate visits and Spine resumptions/depth. Sum-bits additionally passed a full intermediate graph and binding comparison of `both` against `off` with all other modules held fixed.

## Release timings

| Mode | sum-bits inference (ms) | sum-bits whole CLI (ms) | Raytrace inference (s) | Raytrace whole CLI (s) |
|---|---:|---:|---:|---:|
| off | 75.33 | 197.07 | 33.09 | 37.93 |
| siblings | 77.01 | 198.32 | 33.27 | 37.73 |
| parent | 77.16 | 199.83 | 33.27 | 38.48 |
| both | 78.93 | 203.53 | 33.81 | 38.58 |
| local-only | 77.49 | 200.30 | 34.12 | 39.18 |
| parent-check | 77.65 | 209.80 | 33.35 | 38.27 |
| parent-owned | 78.49 | 203.91 | 32.92 | 37.62 |

Measurements on an Apple M3 Max used eight measured blocks for sum-bits and four for Raytrace. The table reports medians. There is one excluded warm-up per case and mode. Each block runs every mode once in a fresh process. Every second block reverses its partner, and the starting order rotates between block pairs. CLI wall time includes loading the artifact, initialization, inference, state export and report writes. Rust compilation and SHA-256 validation are outside the CLI measurement. One Raytrace invocation exhausted temporary disk space during final export. It was excluded and rerun at the same block position.

## Attribution

| Treatment minus control | sum-bits paired Δ (ms) | Raytrace paired Δ (ms) | Raytrace paired differences (ms) |
|---|---:|---:|---|
| both minus off | +2.64 | +403 | +291, +515, +262, +3032 |
| siblings minus off | +0.91 | +713 | -1016, +1763, -338, +1918 |
| parent minus off | +1.85 | -154 | -774, +388, -696, +3894 |
| local-only minus off | +2.27 | +1302 | +1787, +816, -2610, +3233 |
| siblings minus local-only | -2.02 | -183 | -2803, +948, +2272, -1315 |
| parent-check minus off | +1.06 | -133 | +167, -693, -433, +2752 |
| parent-owned minus parent-check | +0.23 | -352 | -874, +1060, -957, +171 |
| parent minus parent-owned | +0.08 | +358 | -67, +21, +694, +972 |

Each contrast subtracts inference time within the same block and then takes the median. Differences between separately reported medians can disagree with this statistic. The joint mode shares checks and the independent contrasts do not form an additive decomposition.

On Raytrace the combined lane was slower in all four blocks, with a median paired increase of 403 ms. The extra Spine sibling checks produced no additional successful wires. They occupy about 55 ms in the sampled proof region. The original local fact decode and acceptance region fell from about 33 ms to 1 ms, while the guarded sibling wiring region used about 49 ms. These regions exclude subsequent bind and seal work. The sibling-only versus local-only release contrast changed sign between blocks, so these data do not establish an end-to-end benefit from bypassing Spine checks alone.

The larger measured cost in the combined lane is resolving parent columns. The proof region occupies about 77 ms. Parent reads account for about 124 ms during RHS execution and 289 ms elsewhere. In the isolated parent lane, those reads occupy about 435 ms against about 48 ms for the corresponding owned values. Keeping the same 153,669 stored parents and switching from owned to projected fields adds a median paired 358 ms in release, with three of four pairs slower. This representation contrast includes discarding owned fields, parent traversal and the resulting cache behavior. The sampled regions account for the cost of the selected operations and cannot assign an exact percentage of the full regression. The full switch remains opt-in.

On sum-bits the combined lane added a paired median of 2.64 ms. The isolated projected-versus-owned contrast was 0.08 ms and changed sign between blocks. The smaller workload shows the same logical work reductions, with insufficient timing separation to rank the individual costs.

## Sampled code regions on Raytrace

| Mode and region | Outermost calls | Timed samples | Mean ns/call | Estimated schedule total (ms) |
|---|---:|---:|---:|---:|
| off / sibling_fallback | 163,551 | 1,288 | 202.1 | 33.0 |
| siblings / sibling_fallback | 4,802 | 38 | 196.3 | 0.9 |
| siblings / sibling_wire | 342,838 | 2,700 | 143.7 | 49.3 |
| siblings / spine_proof | 1,657,700 | 13,053 | 32.9 | 54.6 |
| parent / spine_proof | 317,796 | 2,502 | 111.9 | 35.6 |
| parent / rhs_parent | 560,929 | 4,417 | 236.2 | 132.5 |
| parent / other_parent | 964,752 | 7,596 | 313.0 | 302.0 |
| both / spine_proof | 1,872,925 | 14,747 | 41.3 | 77.3 |
| both / rhs_parent | 560,929 | 4,417 | 220.1 | 123.5 |
| both / other_parent | 964,752 | 7,596 | 299.7 | 289.2 |
| parent-owned / rhs_owned | 560,929 | 4,417 | 14.0 | 7.9 |
| parent-owned / other_owned | 964,752 | 7,596 | 41.5 | 40.1 |

The audit clocks every 127th outermost call in each region and subtracts a back-to-back clock reading. Nested regions are charged to their outermost caller. The estimated total scales the sampled mean by the observed call count. Sampling, work counters and cache state affect these estimates. Use release timings for throughput. The fallback region covers the local fact decode and acceptance check, excluding subsequent binding and sealing. The sibling wire region covers the whole guarded wiring attempt, including rejected attempts. The proof region covers owner qualification, route lookup and optional continuation context acquisition before iterating the consumer atoms. Parent read regions begin after the projected column is selected. Owned read regions begin after finding the field in the map.

For RHS reads, the owned control also resolved the same sampled slot via its pinned parent and verified all 4,417 values. Mean clock-adjusted times were 14.0 ns for the owned value and 636.4 ns for the one-hop replay. The replay runs immediately after the owned read with independent work counters suspended.
For non-RHS reads, the owned control also resolved the same sampled slot via its pinned parent and verified all 7,596 values. Mean clock-adjusted times were 41.5 ns for the owned value and 796.7 ns for the one-hop replay. The replay runs immediately after the owned read with independent work counters suspended.

[Samples, per-block contrasts, phase timings, counters and hashes](slot-path-results.json)

## Reproduce with Cargo

Compile one artifact per workload and reuse it for every mode. Time the release build without the audit feature and a fresh process per sample.

```sh
for case_name in sum-bits raytrace; do
  cargo cell-compile --case "$case_name" --emit-free-join-entry --output "out/paths-$case_name"
  for mode in off siblings parent both local-only parent-check parent-owned; do
    cargo cell-bench --program "out/paths-$case_name/program.json" --lane free-join-planned \
      --modules stage-spine,static-aliases,range-guards --slot-transfer "$mode" --repeat 1
  done
done
```

Use `--verify-stages` with `--slot-transfer` to hold the requested modules fixed while comparing every intermediate graph and binding with transfer off. For a lower-memory ordered RHS comparison on large programs, use the audit feature in fresh output directories.

```sh
export CELL_CPU_FREE_JOIN=planned CELL_CPU_SHARED_NODE_SCHEDULE=1 CELL_CPU_SIEVE=1
export CELL_CPU_JOIN_INTERSECTION=1 CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1
export CELL_CPU_KNOWN_RESULTS=static CELL_CPU_RANGE_GUARDS=1 CELL_CPU_FD_KEYS=0 CELL_CPU_SCALAR_MERGE=0
for case_name in sum-bits raytrace; do
  for mode in off siblings parent both local-only parent-check parent-owned; do
    CELL_CPU_SLOT_TRANSFER="$mode" cargo run --locked --release \
      --manifest-path engine/cell-runtime/Cargo.toml --features slot-transfer-audit -- \
      --program "out/paths-$case_name/program.json" --output "out/audit-$case_name-$mode"
    cmp "out/audit-$case_name-off/rhs.json" "out/audit-$case_name-$mode/rhs.json"
  done
done
```

The whole CLI column times the standalone release executable directly. In the same environment as the audit commands, run the following with a fresh output path. The diagnostic feature is omitted for this build.

```sh
cargo build --release --locked --manifest-path engine/cell-runtime/Cargo.toml
time CELL_CPU_SLOT_TRANSFER=parent "${CARGO_TARGET_DIR:-engine/cell-runtime/target}/release/eggfuse-cell-runtime" \
  --program out/paths-raytrace/program.json --output out/timed-raytrace-parent
```

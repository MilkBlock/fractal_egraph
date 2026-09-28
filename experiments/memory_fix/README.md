# Repair of candidate-set and virtual-row memory amplification

## Status

The two demonstrated representation defects are repaired, and regression tests cover
both. **The whole post-saturation packing pipeline still fails the net-RSS acceptance
criterion against pristine egglog. Do not advertise it as a compression win.**

This is the actual native runtime, not an independent compression model. The graph is
the unchanged 11-round Math workload: **1,047,896 Math enodes / 443,832 eclasses**.
Separate runs check every constructor fact and the equivalence partition modulo IDs.
Existing unrelated working-tree edits were preserved; measurements describe that tested
working tree. The task commits only its own changes.

## Repairs

1. Sparse candidate buffers use shared immutable storage and detach on mutation.
   A query-builder-local cache reuses identical table/constraint evaluations. This
   cache is recreated for each snapshot, so union, insertion and subsume cannot leave
   a previous execution's subset installed as current data.
2. Identical physical root subsets share column indexes. Only roots without remaining
   slow/alias predicates also share child caches. Filtered child results stay separate.
3. Uncovered rows remain in contiguous page buffers. A 256-row physical addressing
   granule uses a bitmap and prefix popcounts to locate virtual versus direct rows.
   **Only virtual rows have recipes/descriptors.** There is no per-uncovered-row Box
   and no per-uncovered-row recipe. Deletion markers use a bitset. Physical row IDs,
   timestamps, native hashcons identity, and merge behavior are preserved.
4. The benchmark now has a `--require-net-rss` acceptance check. Failed net results
   produce a nonzero exit, even if correctness and internal byte accounting improve.

No pattern search budget, representative sampling, or approximate equality was added.
The page size is a storage-addressing granule. A serial collector experiment made the
full workload slower and was reverted; it is not part of the repair.

## Direct evidence

- The 115-atom query's 421.97 MiB of independently retained sparse row-ID copies become
  **3.92 MiB of unique shared candidate buffers**. Reference-based capacities are still
  exposed separately; they must not be summed as independent live allocations.
- For the same isolated query, the diagnostic peak requested Rust allocation drops
  from about **2.04 GiB to 260.58 MiB**, including root-index sharing.
- For the complete round-1 pack, the sum of requested-live allocation deltas *inside
  pack calls*, with their input bindings held constant, changes from **+32.40 MiB to
  −4.47 MiB**. This isolates packing representation changes from discovery queries.
- For 277,803 covered rows, descriptors use **3,333,636 bytes = 12 bytes per virtual
  row**, versus the old 20,447,376 bytes charged across converted tables. This is not
  the complete heap or RSS; binding payload, page buffers, indexes and allocator
  retention still count.
- All 498,217 richer-library fragment matches remain identical to the historical
  complete enumeration. No fewer matches or fewer final graph facts explain savings.

`allocations.json` contains diagnostic samples. Their counting allocator and `ps`
sampling affect execution; they are not used for production timing claims.

## End-to-end pristine comparison — final one-pass screening

System allocator, default native parallelism, release ThinLTO/codegen-units=1. The
pristine binary is hash-verified against its recorded clean `egglog-baseline` build.
Current harness mirrors `native_compare`, adding optional measured library loading,
discovery and packing. No allocator instrumentation, trace or graph export occurs in
the timed processes. Compilation and offline Bake are excluded.

| Variant | Execute + optional library application | Peak RSS |
|---|---:|---:|
| Pristine egglog | 0.294 s | 492.4 MiB |
| Current native, no library | 0.267 s | 363.3 MiB |
| Round-1 library, match only | 0.664 s | 529.9 MiB |
| Round-1 library, pack | 1.512 s | 558.8 MiB |
| Richer library, match only | 5.063 s | 1,533.9 MiB |
| Richer library, pack | 8.976 s | 1,501.1 MiB |

This is a quick single repetition; raw through-drop/process times, library sizes,
binary hashes and full verification results are in `results.json`. The bare-current
kernel's advantage includes earlier changes and must not be attributed entirely to
this patch. The two packed variants **fail** net peak-RSS acceptance against pristine.

The remaining problem is whole-graph fragment discovery: it still builds and discards
query indexes and match bindings for every library fragment, and the allocator retains
resident pages. Packing itself now reduces live allocations in the controlled test,
but by much less than this discovery overhead. The opt-in post-saturation packer remains
a diagnostic path, not evidence that a baked execution planner is faster or smaller.
A runtime that already has certified cell bindings should use those bindings instead
of rediscovering entire saturated bodies; that integration is not claimed by this patch.

## Reproduce and enforce acceptance

```sh
cargo build --offline --release --example post_pack_bench --example virtual_memory_audit
# Reuse the recorded pristine binary, or build from the clean immutable tag checkout:
python3 tools/bench/build_native_compare.py \
  --baseline /Users/mineralsteins/.codex/worktrees/native-baseline/egg_layout
python3 experiments/memory_fix/bench.py \
  --output out/memory-recheck --runs 3 --require-net-rss
```

The last command currently **fails the RSS requirement** after writing its report;
that is intentional, not a passing optimization test. Use a new output directory each
run. The build helper preserves pinned registry versions while permitting the current
path dependency graph to differ from the pristine one, and records binary provenance.

The example `post_pack_bench` supports `MEMORY_FIX_LIBRARY` and `MEMORY_FIX_MODE=match`
for individual runs. Library generation remains documented in `math_pack_rss/README.md`.
The richer library is an existing partial capture, not a claim of complete discovery.

Tests: 56 core tests and 40 integration tests passed; one old explicit timing test is
ignored. New tests enforce shared candidate storage, update/union/subsume freshness,
separate alias filters, sparse descriptor cost, and bitmap rank across words, page
boundaries and repacking. Existing virtual storage, cell-machine, DABCABC and branching
reentry tests remain covered.

# GPU union repair with complete block summaries

This changes the actual resident Metal engine, not the standalone CPU executor
or its Sieve query filters. The ordinary GPU compilation path is unchanged.
Enable it explicitly through the normal compiler pipeline:

```sh
python3 -m eggfuse compile --source input.egg --output out/compiled \
  --build out/device-build --solver-python /path/to/solver-python \
  --multi-stage-cell --incremental-rebuild
```

The compiler emits the summaries' physical layout in `program.json` under
`incremental_rebuild`. Its new lowering module and Metal source are dependencies
of the device-pass cache. Existing artifacts and default compilation retain the
old kernels and layout. The low-level `lower(..., incremental_rebuild=True)`
selects the new path; `False` compiles the same new shader with filtering disabled
for a paired ablation, and `None` leaves the original path unchanged.

## State and execution

Every 256 physical row positions share a complete 1,024-bit summary of the
**stored equality IDs**, including both keys and outputs. The summary is built
from actual rows, not sampled Bake matches. It does not replace the authoritative
rows or union-find and does not select representatives.

A successful union adds the losing root ID to a pending displacement signature.
Both the serial union implementation and the atomic parallel Cell union path
publish this event. At each rebuild iteration the pending signature is frozen;
new unions caused by merge callbacks accumulate separately for the next one.

A block enters exact canonicalization if it is new, dirty, changed in length,
contains container fields, or its signature intersects the displacement
signature. Otherwise the block cannot reference a displaced root and is skipped.
The hit case still performs the original exact root lookup and updates the
original row, fact revision, table version and timestamp. Bloom collisions only
increase work; they cannot authorize equality or effects.

Existing function rows only change their result field. Scalar-result changes
therefore preserve the ID summary. Equality/container-result changes invalidate
the block. Appended rows are detected by block length/initial validity. Row
compaction rebuilds summaries for relocated live blocks and **invalidates all
evacuated blocks**. This last requirement matters when future Unit/scalar rows
refill exactly the same physical range; equal block length is not evidence that
the old summary still describes the contents.

Container blocks conservatively enter every repair. Rebuild still canonicalizes
container storage before inspecting the parent rows. This implementation does
not pretend that hashing a container handle summarizes its nested equality IDs.

Once a complete repair established key uniqueness, a subsequent iteration can
skip duplicate-key grouping when there are no appended rows and no key fields
changed. Output-only ID changes still update values/versions. If any key changes,
including same-handle container changes, the original grouping and stable merge
loop run. Custom merge order and its intermediate effects are not altered.

## What remains global

- Hit blocks still rewrite affected stored IDs. This is not a lazy-ID graph in
  which all old row identifiers can remain forever.
- Grouping after key changes, query-index maintenance, compaction, and custom
  merge callbacks retain the existing implementation. The generic callback loop
  is still ordered per arena.
- Broad unions can make the displacement signature dense; Bloom false positives
  then remove much of the scan reduction. There is no candidate budget or
  incomplete scan fallback.
- Summary storage is proportional to allocated row blocks, not Cell plans or
  row × plan pairs. The current representation costs 328 bytes per block plus a
  small per-arena header/signature. Counts and validity bits are included.
- Initial summaries and compaction maintenance cost work. Fewer visited row
  positions alone is not a throughput result.

The representative policy, canonical hashcons keys, source finite Repeat /
Saturate control, delayed union boundary, and multi-stage Cell completion remain
unchanged. The GPU retains all state; the CPU does not route individual union
notifications. Existing Run-boundary checkpoints include the new buffers and
pending signatures through the declared arena layout.

## Validation and reproduction

```sh
# Original native endpoint, on/off complete GPU states and per-Run fingerprints,
# plus forced Bloom collisions (same shader).
python3 experiments/staged_batches/check_incremental_rebuild.py \
  --build out/eggfuse-clean-paths-build --output out/repair-check

# New-state checkpoint persistence, saved continuations and fresh-worker replay.
python3 experiments/staged_batches/check_checkpoint.py --incremental-rebuild \
  --build out/eggfuse-clean-paths-build --output out/repair-checkpoint

# Real frozen Pass 3, normal multi-stage Cell execution. No native pass 1/2 rerun.
python3 experiments/staged_batches/check_incremental_rebuild.py --bench \
  --pass3 fact loopfact sum-bits --build out/eggfuse-clean-paths-build \
  --output out/repair-pass3
```

The Pass-3 benchmark performs one excluded compilation/first-launch preflight,
then one cached off run and one cached on run. Both measured variants use the
same worker and shader. The preflight state must match the measured control;
all final facts, IDs, values, visibility, control and Run histories must match
across the measured pair. Shader setup is outside inference timing. Operational
repair counters remain enabled in both variants and their cost is included.
Do not compare these timings directly to the historical CPU-assisted 12-second
configuration or attribute them to Cell proof coverage.

Tests include late union, finite visibility, recursive generation, mutable
function replies, merge-induced unions, subsumption, containers, sparse unions,
output-only renaming and physical compaction followed by regrowth. The regrowth
case specifically exercises evacuated blocks being reused by different Unit
relations. Independent native checks cover all these small sources. Large
Pass-3 comparisons retain the same resident-executor semantics; they are not a
new pristine-native whole-program equivalence proof.

## 2026-10-31 results and current decision

The feature remains **opt-in**. Ten native-checked fixtures pass with the feature
off/on and all Bloom keys deliberately colliding. Four physical checkpoint cases
pass with `--incremental-rebuild`, including pending continuations, union,
custom merge, subsumption and dynamic buffers. Four compiler schedule/Cell-wave
unit tests also pass. The default lowered program/shader was compared with
`9d66e57` on a real late-union source and is identical.

One final, complete frozen **fact Pass 3**, after an excluded first-launch preflight:

| Metric | Off | On |
|---|---:|---:|
| Inference | 278.391 ms | 285.354 ms |
| GPU command-buffer time | 196.153 ms | 198.211 ms |
| Canonicalization row positions visited | 18,935 | 14,830 |
| Blocks skipped | 0 / 90 | 17 / 90 |
| Duplicate-group row positions skipped | 0 | 4,860 |
| Logical Runs | 407 | 407 |
| Additional summary storage per arena | 84,260 B | 84,260 B |

All exported rows, IDs, values, visibility, parents, watermarks, control, and
per-Run histories match. This pair shows **no net speedup** (about 2.5% slower
inference in this single run). Lower scan counts are not a substitute for net
performance. No statistical significance or slowdown attribution is claimed from
one subsecond pair. Full row/index maintenance and ordered merge remain, while
summaries themselves incur maintenance. The retained off mode allocates the same
summary space and uses the same shader for the ablation; ordinary compilation
with no switch still uses the original layout/kernels.

The cold preflight spent 280.45 seconds compiling shader pipelines and is not
used as the timing control. Earlier exploratory results, including a partially
completed three-case suite, preceded the evacuated-block fix and are excluded.
There is **no new Raytrace performance result**, and loopfact/sum-bits have not
been retimed on the final implementation. Their frozen inputs remain supported
by the reproduction command above. Sources and compact text evidence are in
`incremental-rebuild-results/`; generated shaders, workers and checkpoints remain
untracked under `out/gpu-incremental-rebuild/`.

# Remove capacity-sized serial scratch clearing

The new resident engine uses a serial ordered rebuild to preserve merge effects.
Inside each rebuild iteration it formerly cleared every duplicate-group hash
bucket: `HASH_CAP` entries, regardless of the number of live rows. With the Pass-3
allocation capacity, even a tiny union graph pays millions of serial stores.

The resident arena already allocates per-bucket generation tags for its query
index. Rebuild owns the same head scratch area until it returns. It now advances
that generation and initializes a bucket only when probing it. The next query
reset advances the generation again. Collisions retain their original linear
probe order, duplicate groups and merge ordering. Generation overflow explicitly
clears tags. No new buffer, heuristic, smaller Bake library, or inference omission
is introduced. Nonresident/QUERY_PATCH paths retain the original behavior.

2026-10-30, one run per variant, same worker binary and source program:

| Mode | GPU diagnostic | Inference diagnostic |
| --- | ---: | ---: |
| Capacity-wide clear | 266.98 ms | 274.44 ms |
| Initialize visited buckets | 21.84 ms | 29.43 ms |
| Forced generation wrap | 38.43 ms | 46.02 ms |

This is the actual Metal engine, with allocation capacity 1,048,576, 97 initial
rows, 129 final rows, 32 unions, 33 Runs. Every final state field and every Run
fingerprint/counter agrees across variants. The wrap case deliberately pays one
full tag reset. These are checked local diagnostics, **not a full Pass-3 speedup**.
Credit belongs to executor scratch maintenance, not Cell coverage or proof reuse.

Six additional custom-merge, compaction, visibility, independent-union and overflow
regressions pass (each repeated on GPU; overflow also fails in native as expected).
Results: `out/resident-sparse-rebuild/result.json` and
`out/sparse-rebuild-contracts/results.json`.

```sh
python3 experiments/staged_batches/check_sparse_rebuild.py \
  --build out/eggfuse-clean-paths-build --output out/rebuild-check
```

# C3 shared ClosedState storage probe

This is a **standalone Python storage prototype**, not a change to egglog's tables,
matcher, or rewrite execution. Native `ripen-use` supplies the symbolic closures.

## Representation and correctness

- Flat: globally canonicalized, deduplicated `Add(lhs, rhs, result)` tuples in an
  `array('Q')`, union-find, and a left-argument lookup index.
- Shared: one 12-row C3 template, seven global handle bindings per instance
  (including internal/output eclasses), residual tuples, union-find, and a
  left-argument-to-instance index. Instances whose entire row set is already
  covered are discarded; partially overlapping instances remain.
- RipenInput rows and their scalar labels are parameter markers, not stored global
  constructors. Original historical tokens, qualified by history path, bind both
  parameters and original outputs. Unbound internal classes receive fresh handles.
  Exported `symbolic_values` ground terms are resolved in the source closed graph,
  then translated through the catalog's exact `value_map` to representative slots.
- Union triggers congruence closure of the stored Add rows and full index rebuild.
  Insertions use the residual table. This prototype does not run rewrite rules.
- All facts, all bound-left queries, and the external consumer join
  `Add(x,y,z), Add(z,w,t)` are compared with the flat store before and after each
  residual insertion/union. Randomized tests exercise additional overlaps.

The instantiated global store is **not certified Closed**. Binding substitution,
additional facts and unions can enable new matches. Original whole-tier0 facts
and equalities outside the selected exports are not loaded. Therefore this is a
measurement over the union of extracted symbolic closures, not the whole tier0
memory reduction. Congruence in both arms deduplicates that union fairly.

## Recorded results

100 C3 triggers from math6, previously from 12 source templates, re-ripened with
all 24 source rules, four rounds; 100 Closed, one exact shared state. The compact
input `c3.json` preserves template, instance bindings, handle count and aliases.

| Measurement | Flat | Shared |
| --- | ---: | ---: |
| Unique Add facts | 304 | 304 (virtual) |
| Retained template instances | n/a | 39 of 100 |
| Fact/template/binding payload | 7,296 B | 2,472 B |
| Retained objects including UF/index | 25,740 B | 20,540 B |
| Construction (median) | 4.15 ms | 5.52 ms |
| Query every global handle (median) | 0.687 ms | 7.096 ms |
| External consumer join (median) | 0.592 ms | 4.474 ms |

Payload falls 66.1%, but **total retained object bytes fall only 20.2%**. The
1,200 pre-deduplication Add occurrences are NOT the flat baseline. Three unions
reduce unique facts to 245, 217, 164; both stores remain identical. Shared union
rebuilds take about 2.2–3.7 times the flat rebuilds in this run (single samples).

Bytes use recursive `sys.getsizeof` on owned slots, arrays, UF and index, including
array capacities. They exclude interpreter/code, allocator metadata, temporary
build/query materializations, and source evidence shared outside both stores.
This is neither RSS nor peak memory. Times are Python implementation timings,
not a prediction of Rust or egglog performance. Querying currently expands each
candidate instance, explaining the slowdown. A template-level slot/row index or
batched template consumer would be the next experiment; acceleration is unproven.

## Reproduce

From the repository root, replay the committed compact input (no trace required):

```sh
python3 tools/probe_closed_storage.py \
  --dataset experiments/closed_storage/c3.json \
  --output out/closed-storage-replay.json
python3 -m unittest discover -s tools -p test_closed_storage.py
cargo test --release --test ripen --test closed_state
```

Rebuild native provenance from the existing survey (choose a fresh output path):

```sh
cargo build --release
python3 tools/prepare_closed_storage.py out/closed-survey/catalog/catalog.json \
  --state 3 --output out/closed-storage-refresh
python3 tools/probe_closed_storage.py \
  out/closed-storage-refresh/catalog/catalog.json --state 0 \
  --output out/closed-storage-refresh/storage.json
```

The refresh requires the original histories named by the survey. It rejects
changed Use membership, unsuccessful closure, unsupported fact operators, or
unresolved token terms. Only Add/i64 marker expressions are supported in this
bounded probe; the restricted reader is not a replacement egglog parser.
`results.json` contains the complete measurement and post-mutation checks.

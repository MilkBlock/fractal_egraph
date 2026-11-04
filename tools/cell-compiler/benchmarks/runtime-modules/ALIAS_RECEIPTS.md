# Forwarding source witnesses for skipped constructors

With static constructor aliases and stage Spine active together, an eligible skipped RHS constructor now supplies its original physical witness to the anchor publisher. The typed plan derives both the result slot and its source from the existing full-key proof. An alias keeps pointing to the first matched query atom or earlier, non-elided constructor with that key, including when an input was itself statically aliased. Loading older frozen artifacts derives this mapping from the same validated recipe. The emitted artifact format stays compatible.

For an eligible producer the compiled anchor layout identifies which query witnesses to carry. A single required query atom uses inline storage; rules needing multiple atom positions use a vector. Earlier RHS witnesses come from the existing action receipt array. Publication coalesces canonical keys in source order, follows same-key repair forwards, and checks target table and visibility. Missing witnesses and rejected references fall back to exact keyed lookup. Equal result classes alone cannot identify a constructor row.

## End-to-end comparison

| Frozen Pass 3 | Publication fallbacks, control → forwarded | Median inference | Paired time wins | Requests / Cells | Resumptions / depth |
|---|---:|---:|---:|---:|---:|
| sum-bits | 1,110 → 58 | 75.91 ms → 75.78 ms | 4/7 | 11,774 / 9,492 | 3,034 / 12 |
| raytrace | 140,110 → 29 | 33.64 s → 33.53 s | 3/5 | 2,096,703 / 3,741,283 | 663,403 / 29 |

Both policies use the same frozen program and run the standalone Cell CPU executor with static aliases, lazy RHS frames, planned Free Join, Sieve, column intersection, shared entries and first-resumption Spine with receipts. The control is `4277977`. The candidate changes how the receipt is obtained. Each lane received an excluded warm-up followed by seven alternating paired rounds on sum-bits or five on Raytrace. A fresh process constructs a fresh graph for each sample. The timed interval is the complete inference schedule, excluding initialization, final export and audits.

On Raytrace the difference between lane medians was -0.33%, with a median paired change of -0.29 s. Three of five pairs were faster. On these samples the throughput difference is small relative to the variation between runs.

Before within-Run coalescence, Raytrace forwarded 169,770 matched query references and 57,583 references from earlier RHS operations. Successful post-coalescence receipts increased from 2,162,061 to 2,302,142. Missing receipts went from 140,081 to zero, while rejected receipts stayed at 29. Total graph key lookups decreased from 4,537,862 to 4,397,781. The corresponding sum-bits source counts were 1,609 and 335, with 58 final rejections and zero missing receipts. Key lookups fell from 20,240 to 19,188.

Both versions eliminate 1,222,040 Raytrace constructor requests, avoid 134,085 RHS frames and visit 9,522,225 candidates. On sum-bits they eliminate 5,853 constructor requests, avoid 966 frames and visit 15,549 candidates.

## Validation and reproduction

Each timed sample matched the frozen full endpoint, scheduling digest and effect counters. An independent untimed audit compared each Run's SHA-256 digest of the ordered `(rule ID, slot/value map)` sequence in the sealed RHS patch, including empty Runs. The old and new arrays match element by element on both workloads. Sum-bits also matched the initial graph and every stage graph and ordered binding snapshot against the Cargo control. Intermediate full Raytrace graph snapshots were outside this audit. A focused regression covers two different constructor keys with equal result classes, hiding one source before publication, and an RHS receipt reused through a normalized input alias.

```sh
cargo cell-compile --case sum-bits --emit-free-join-entry --output out/alias-sum
cargo cell-compile --case raytrace --emit-free-join-entry --output out/alias-ray
cargo cell-bench --program out/alias-sum/program.json --lane free-join-planned --modules stage-spine,static-aliases --repeat 7 --verify-stages
cargo cell-bench --program out/alias-ray/program.json --lane free-join-planned --modules stage-spine,static-aliases --repeat 5
```

Run these same benchmark commands against the same programs on the control revision and candidate. To reproduce the original timing protocol use `--repeat 1` in rotated fresh processes. For the independent ordered-RHS audit apply [the diagnostic patch](root-rhs-audit.patch) in temporary copies of both revisions and use the [static runtime invocation](STATIC_RHS.md#validation-and-pure-cargo-reproduction) with `CHECK_RHS_DIGESTS=1` and compare the resulting `rhs.json` files with `cmp`. [Per-round timings, counters and digests](alias-receipt-results.json)

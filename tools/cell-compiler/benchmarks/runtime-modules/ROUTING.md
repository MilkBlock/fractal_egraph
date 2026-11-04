# Routing shared entries to Spine and the ordinary matcher

When a shared entry node refreshes, its consumer sees the current delta packet and watermark. A Spine route requires both a compiler-derived exhaustive functional-key order for the specific rule and pivot and a live cursor for the exact committed witness. Other packets from the same node or rule retain the ordinary entry policy. This uses the existing artifact format, stage Spine switch and source Run boundaries.

A necessary Sieve or reverse-domain check can discard an impossible packet before either route. For remaining packets, Spine executes the complete key order, checking current ports or refreshing an absent or stale port by exact key. Unclaimed packets use the configured Free Join admission, singleton delta ownership check and residual solver. Only claimed packets participate in cursor depth tie-breaking. A query returns early when all delivered packets were claimed. Mixed queries merge their ordered Spine answers with the ordinary matches through the existing binding deduplicator. A constructor-only RHS with no producer or resumed cursor can use the existing lazy frame path.

For example, a consumer joining `A(x)` with `B(x)` can have both a committed producer cursor for `A(1)` and an ordinary bootstrap witness `A(2)`. The first can resume its producer's Spine. The second can still probe its partner using the planned entry. The regression includes a third key with a late partner, the removal of the first anchor, forced index fingerprint collisions and the receipt path.

## Frozen Pass 3 comparison

| Executor | sum-bits, ms | Raytrace, s |
|---|---:|---:|
| Spine off | 76.10 | 32.98 |
| Previous Spine | 111.06 | 54.40 |
| Routed Spine | 104.66 | 48.41 |

| Input / lane | Delivered seeds | Legacy candidate visits | Physical entry probes | Decoded rows | Resumed steps | Maximum chain |
|---|---:|---:|---:|---:|---:|---:|
| sum-bits / old | 58,905 | 16,532 | 0 | 14,716 | 3,034 | 12 |
| sum-bits / new | 12,788 | 15,549 | 14,057 | 12,005 | 3,034 | 12 |
| sum-bits / off | 10,326 | 15,549 | 16,537 | 13,081 | 0 | 0 |
| raytrace / old | 33,476,641 | 9,828,851 | 0 | 8,088,770 | 663,403 | 29 |
| raytrace / new | 4,577,790 | 9,522,225 | 2,768,996 | 7,486,641 | 663,403 | 29 |
| raytrace / off | 3,437,773 | 9,522,225 | 3,155,529 | 7,862,010 | 0 | 0 |

The control and routed lanes use the same updated release binary. The previous Spine uses the release binary from #17 with identical flags and input. All three use the planned Free Join lane, Sieve, column intersection, shared nodes, ordinary storage and default merge programs. Only the Spine switch differs between the control and routed lanes. The receipt and partial suboptions are off. One warm-up per lane precedes seven rotated rounds on sum-bits and three on Raytrace. Each sample starts a fresh process and graph. One previous-Spine Raytrace sample that overlapped a compiler build was replaced by an isolated run before resuming the matrix. Timings cover schedule inference only.

The legacy candidate count sums shared-node predicate checks, direct and residual candidates, merge candidate rows and Spine pivot candidates. Entry index probes, decoded rows and seed deliveries are separate work measures. On Raytrace, 1,872,925 delivered seeds went to Spine and 2,704,865 to the ordinary matcher. There were 722 rule invocations where both routes received packets. On sum-bits the corresponding figures are 5,785, 7,003 and 111. These counts describe routed packets and mixed rule invocations. A packet can be refuted by its consumer and multiple packets can produce the same RHS inputs.



| Raytrace phase median | Spine off | Previous Spine | Routed Spine |
|---|---:|---:|---:|
| Search | 16.08 s | 23.15 s | 17.40 s |
| RHS | 9.17 s | 13.56 s | 13.37 s |
| Query updates and publication | 4.73 s | 14.70 s | 14.68 s |

Spine still performs 6,002,351 capture lookups on Raytrace and 16,425 on sum-bits. The production default keeps Spine off. The measurements above isolate the new entry routing and the related lazy RHS eligibility change from the separate receipt optimization.

## Validation and reproduction

The small frozen workload matched the initial graph and all 1,428 Run graphs, ordered binding records and schedules against the same binary with Spine off. Every timed sample matched the frozen full graph endpoint and per-Run scheduling and binding counts. Separate untimed Raytrace builds hashed each ordered `(rule_id, slot_values)` pair from the prepared RHS patch, using SHA-256 per Run. The old and routed Spine matched on all 16,883 ordered binding digests and the byte-for-byte final graph. Intermediate full Raytrace graphs were outside that audit. The focused fixture also matched its uninterrupted final graph after restoring a checkpoint with cold Spine contexts.

```sh
cargo cell-compile --case sum-bits --emit-free-join-entry --output out/route-sum
cargo cell-bench --program out/route-sum/program.json --lane free-join-planned --repeat 7
cargo cell-bench --program out/route-sum/program.json --lane free-join-planned --modules stage-spine --repeat 7 --verify-stages
cargo cell-compile --case raytrace --emit-free-join-entry --output out/route-ray
cargo cell-bench --program out/route-ray/program.json --lane free-join-planned --modules stage-spine --repeat 3
```

Use the same frozen program and module command at commit `7f0daf9` for the previous Spine. [Raw samples, phase medians, counters and fingerprints](routed-results.json)

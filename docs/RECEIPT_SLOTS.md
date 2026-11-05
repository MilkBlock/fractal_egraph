# Publishing structural ports on the producing receipt

The experiments use the standalone single-thread Cell CPU engine, the original frozen 64-unit sum-bits and Raytrace schedules, and the 16 structural paths trained on the other workload. Both the control and the trials target the same existing Spine LHS table reads. The compiler and the runtime use typed emitters derived from the selected paths. No operator name, rule ID or workload name selects an execution path.

## What changes at the Run barrier

The previous implementation built an ordered packet for every candidate and selected the last writer for each canonical anchor after repair. A proposal may be empty. The proposal census compares against the most recent proposal on the raw anchor in the same Run, or its previously published ports. A separate census compares the winning packet against the published ports of its repaired anchor. Each count is a packet update for one anchor in one Run.

| Quantity | sum-bits | Raytrace |
|---|---:|---:|
| Candidate proposals | 5,312 | 1,516,143 |
| Identical nonempty / empty proposals | 1,186 / 1,012 | 844,972 / 203,248 |
| Superseded on the same raw anchor | 1,735 | 727,390 |
| Distinct raw winners redirected into competing receipts | 54 | 9,820 |
| Canonical winners | 3,523 | 778,933 |
| Install / replace / clear a sealed packet | 870 / 829 / 180 | 78,320 / 83,118 / 15,951 |
| Sealed packet already identical or empty | 1,644 | 601,544 |
| Previously populated anchors retired separately | 171 | 25,095 |

These are counts of logical proposals and ordered receipt copies. Most small temporary packets fit inline. A discarded proposal does not imply a saved heap allocation.

The first trial (`bbe6a63e`) puts a four-byte handle in the constructor fact or function receipt. It retains the latest proposal ordinal even for repeated writes and changes only the winning sealed value. The slot arena holds nonempty packets and reuses handles after retirement. Collection still builds the full candidate packet at every RHS. On Raytrace its capture and publication profile measured 527 and 201 ms, versus 302 and 332 ms for the previous map with the same census enabled.

The second trial compiles each emitter's distinct receipt operands into numbered inputs. A producer queues the emitter and those input references. At the barrier it resolves same-key forwarding, chooses the latest source ordinal, and then evaluates each winning emitter against the *previous* sealed slots. It installs the differences only after all evaluations. Deleted or hidden anchors detach their slot into a Run-local retirement list so older inputs remain readable up to this barrier. A consumer rechecks the target's visibility and full canonical key before reusing it. A later empty publication wins on a canonical collision. The regression fixture reverses initial allocation order to test both an empty and a populated final winner.

On Raytrace the second trial gathered 1,727,628 input references, ran 778,933 port programs and changed 177,389 packets. This removes 737,210 candidate packet evaluations. Peak retained ports and actual replaced probes matched the previous plan on both inputs. Raytrace retained at most 48,166 port references and replaced 303,355 probes. Sum-bits retained 560 and replaced 155.

## End-to-end comparison

| Workload | Inference, off / previous map / deferred slots | Full CLI, off / previous map / deferred slots | Paired deferred minus previous inference |
|---|---:|---:|---:|
| sum-bits, 9 rounds | 32.59 / 33.19 / 33.46 ms | 138.22 / 141.01 / 142.28 ms | +0.13 ms, faster in 4/9 |
| Raytrace, 5 rounds | 12.490 / 13.370 / 13.197 s | 14.882 / 15.783 / 15.707 s | −173 ms, faster in 4/5 |

These are medians of the final alternated runs without simultaneous builds or other trials from this experiment. Against off, Raytrace's median paired inference difference was +707 ms. The earlier exploratory matrix had larger timing variation. The receipt-only eager trial also failed to beat off. The option stays disabled by default.

The JSON alongside this document contains separate paired matrices for eager publication and deferred publication, all inference and CLI samples, separate profiles, workload fingerprints and correctness digests. The CLI clock includes loading and export. The elapsed inference clock includes search, RHS, repair, publication and collection.

## What the consumer actually saves

With `slot-transfer-audit` and `CELL_CPU_PROFILE=1`, the selected Spine read runs both the port check and ordinary canonical lookup on each requested key. It alternates which read goes first and verifies that every hit returns the same live physical witness and row. A miss also requires the canonical lookup in normal execution. The timings share caches and include timer overhead. They diagnose the value of replacing these specific reads. Use the uninstrumented paired runs for performance claims.

On Raytrace, 303,355 of 1,576,013 attempts supplied the requested row (19.25%). In two diagnostic runs, matching rows cost 121–137 ms in ordinary table reads and 58–67 ms through slots. The 1,272,658 unsuccessful slot checks cost a further 58–67 ms before their fallback. The aggregate read-side difference (canonical time on hits, less slot time on all attempts) was 4.08 and 5.36 ms. An independent profile of the deferred trial spent 139 ms gathering source receipts and 385 ms at publication. The read-side cost estimate compares operations on the same keys. It cannot be directly subtracted from uninstrumented CLI times.

This catalog already recovers nearly all the covered lookup volume of the broad experiment. Future selection should rank a block by physical registrations, index updates, candidates or complete RHS frames that it can demonstrably eliminate. The 69,732 complete non-resumed RHS applications in the previous broad Raytrace trial provide candidates for that measurement. Each must still satisfy publication visibility, union order and externally observable consumers.

On both workloads, per-Run RHS inputs, effective write order, resumptions, Spine shapes, schedule and exported graph matched the frozen control digests. Sum-bits executed 1,428 Runs and 3,034 resumptions, longest chain 12. Raytrace executed 16,883 Runs and 663,403 resumptions, longest chain 29.

## Reproduce

From the branch root, generate the two frozen programs and the opposite-workload catalogs with the Cargo commands in *Cross-Run completion with deeper producer structure*. Use a fresh output path each time.

```sh
cargo run --release --locked -p eggfuse-cell-runtime --features receipt-slots,view-ports -- \
  --program out/raytrace/program.json --structural-depth 3 \
  --structural-catalog out/train-sum/structural-catalog.json --output out/ray-slots

cargo bench --locked -p eggfuse-cell-compiler --bench cell --features receipt-slots,view-ports -- \
  --program out/raytrace/program.json --structural-depth 3 \
  --structural-catalog out/train-sum/structural-catalog.json --repeat 5
```

For the prior map on the same input, replace `receipt-slots` with `structural-depth`. For the off control, use that same map build at depth zero without a catalog. For the publication census and paired lookup diagnostics, run the CLI with `CELL_CPU_PROFILE=1` and add `slot-transfer-audit,spine-shapes` to its features. The frozen reference hashes and raw timings are in the results file. The receipt slot feature stays opt-in.

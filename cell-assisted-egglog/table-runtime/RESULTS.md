# Release benchmark results

Core time = setup + input + all driver rounds, including marker maintenance, planning, transfer and commit. Each variant is one fresh process. Verification runs separately; graph/value/visibility equality is checked against pristine.

| Case | Instances | Expr nodes / classes | pristine core (s) | fork core (s) | marker-paired core (s) | device-tables core (s) |
|---|---:|---:|---:|---:|---:|---:|
| local | 16 | 64 / 48 | 0.004481 | 0.004784 | 0.323578 | 0.102220 |
| local | 256 | 1024 / 768 | 0.025588 | 0.027108 | 0.270450 | 0.093134 |
| local | 4096 | 16384 / 12288 | 0.652695 | 0.689717 | 5.813500 | 0.819556 |
| sparse-cycle | 16 | 96 / 80 | 0.010676 | 0.012304 | 0.197149 | 0.111249 |
| sparse-cycle | 256 | 1536 / 1280 | 0.043667 | 0.045773 | 0.260563 | 0.110028 |
| sparse-cycle | 4096 | 24576 / 20480 | 1.185977 | 1.228402 | 2.654936 | 1.375391 |

## Largest cases: measured run phases

| Case / variant | Run (s) | Admission (s) | CPU routing (s) | Installation (s) | Dispatch (s) | GPU device (s) |
|---|---:|---:|---:|---:|---:|---:|
| local / 4096 / marker-paired | 5.116509 | 0.049848 | 3.944394 | 0.984036 | 0.105428 | 0.037213 |
| local / 4096 / device-tables | 0.073460 | 0.048783 | 0.000000 | 0.000000 | 0.011940 | 0.000817 |
| sparse-cycle / 4096 / marker-paired | 1.407418 | 0.077394 | 0.963461 | 0.338577 | 0.020077 | 0.003408 |
| sparse-cycle / 4096 / device-tables | 0.102241 | 0.077734 | 0.000000 | 0.000000 | 0.013275 | 0.000884 |

GPU device time is a subset of dispatch; do not add these columns. Static table shader compilation is in setup. These are controlled workloads using an analyzed Single/Get Cell, not a complete eggcc pass.

Library files: 20,244 bytes. RSS measurements are sampled sums of currently live processes and can miss peaks or count shared pages twice. They are not egraph compression ratios.

See `results.json` for exact graph sizes, primitive verification, phase breakdowns, binary hashes and source provenance.

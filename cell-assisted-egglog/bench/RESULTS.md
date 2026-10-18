# Release benchmark results

Core time = setup + input + all driver rounds, including marker maintenance, planning, transfer and commit. Each variant is one fresh process. Verification runs separately; graph/value/visibility equality is checked against pristine.

| Case | Instances | Expr nodes / classes | Pristine core (s) | Fork core (s) | Marker paired core (s) | Marker solver core (s) |
|---|---:|---:|---:|---:|---:|---:|
| local | 16 | 64 / 48 | 0.004843 | 0.004931 | 0.476232 | 0.349303 |
| local | 64 | 256 / 192 | 0.007279 | 0.007691 | 0.271783 | 0.343916 |
| local | 256 | 1024 / 768 | 0.026300 | 0.027808 | 0.577644 | 1.272996 |
| local | 1024 | 4096 / 3072 | 0.119789 | 0.152863 | 1.873795 | 38.433949 |
| local | 4096 | 16384 / 12288 | 0.730106 | 0.749032 | 11.249577 | 20.262343 |
| sparse-cycle | 16 | 96 / 80 | 0.011796 | 0.012288 | 0.228466 | 0.382174 |
| sparse-cycle | 64 | 384 / 320 | 0.011359 | 0.011849 | 0.360844 | 0.511286 |
| sparse-cycle | 256 | 1536 / 1280 | 0.043681 | 0.046902 | 0.842045 | 0.989035 |
| sparse-cycle | 1024 | 6144 / 5120 | 0.210714 | 0.216417 | 2.765847 | 2.975815 |
| sparse-cycle | 4096 | 24576 / 20480 | 1.280441 | 1.322266 | 12.046060 | 12.619742 |

## Largest paired cases: measured phases

| Case | Run (s) | Marker routing (s) | Installation (s) | Worker JSON decode (s) | Shader compilation (s) | GPU device time (s) |
|---|---:|---:|---:|---:|---:|---:|
| local / 4096 | 10.499874 | 4.162963 | 6.147533 | 4.666899 | 0.004004 | 0.030796 |
| sparse-cycle / 4096 | 10.783704 | 0.892157 | 9.776728 | 9.235475 | 0.003506 | 0.002614 |

Decode/compilation are subsets of installation; device time is a subset of dispatch. Do not add these columns. Timings show no pristine speedup. These are controlled workloads using an analyzed Single/Get Cell, not a complete eggcc pass.

Library files: 20,244 bytes. RSS measurements are sampled sums of currently live processes and can miss peaks or count shared pages twice. They are not egraph compression ratios.

See `results.json` for exact graph sizes, primitive verification, phase breakdowns, binary hashes and source provenance.

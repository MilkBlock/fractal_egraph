# Plan Free Join at shared entries

The `free-join-entry` lane considers the complete typed LHS before seed delivery. For each eligible semi-naive pivot, the compiler identifies the columns supplied to each connected table atom by that pivot and by source literals. It interns equal projection descriptors and prioritizes subatoms with more constrained columns. Partial and full keys are both legal. Disconnected atoms and primitive predicates remain with the regular matcher. The existing residual Free Join lane is the paired control.

```sh
cargo cell-compile --case sum-bits --emit-free-join-entry --output out/early-sum
cargo cell-bench --program out/early-sum/program.json --lane free-join --repeat 7
cargo cell-bench --program out/early-sum/program.json --lane free-join-entry --repeat 7 --verify-stages
```

The optional `free_join_entry` section is tied to the Recipe digest; loading checks its rule, atom and column wiring against the complete typed query. Older artifacts derive the same plan at executor construction when this lane is requested. Frozen sum-bits has 3,018 planned probes across 1,688 pivot positions using 679 projection descriptors.

For example, in `R(x,y), S(y,z), T(z,x)`, the R delta row can probe S on y and T on x before allocating a consumer seed. After Sieve admission, an empty projected domain rejects the seed. A unique possible visible witness can also disqualify the pivot when the original semi-naive epoch, witness and atom tie-break assign this match to another entry. Successful subtries accompany the seed into the matcher. Point reads can use a singleton child, and additional bindings can select a tighter column index. Exact values, visibility, delta ownership and the original RHS are checked as before. Cached results are keyed by projection and column fingerprints and are scoped to one Run. Fingerprint collisions may enlarge a child domain. They cannot certify a match.

## Same-program comparison

| Frozen input | Candidate visits, residual → entry | Delivered seeds, residual → entry | Residual FJ, ms | Entry FJ, ms | Entry faster pairs |
|---|---:|---:|---:|---:|---:|
| fact | 3,247 → 3,127 | 6,072 → 2,186 | 15.67 | 15.54 | 7/9 |
| loopfact | 2,900 → 2,770 | 5,555 → 1,919 | 15.57 | 14.92 | 5/9 |
| sum-bits | 16,286 → 15,549 | 32,792 → 10,326 | 76.70 | 72.62 | 7/9 |
| dot-product | 14,399 → 13,838 | 27,104 → 8,549 | 62.19 | 60.82 | 7/9 |
| sum-divisors | 40,959 → 39,244 | 79,803 → 25,016 | 176.69 | 178.10 | 6/9 |
| fenwick_tree | 44,964 → 43,144 | 100,859 → 26,581 | 210.90 | 199.95 | 7/9 |
| jacobi-1d | 90,896 → 87,432 | 185,826 → 49,982 | 407.87 | 392.23 | 8/9 |
| raytrace | 9,748,963 → 9,522,225 | 14,526,197 → 3,437,773 | 36,668.80 | 35,483.26 | 3/3 |

The workload table retains the previous candidate metric; entry probe work is reported separately. On Raytrace the entry lane performed 12,350,778 index probes, reused 2,490,930 same-Run probe results and avoided 2,380,780 graph key lookups relative to residual-only Free Join. It rejected 10,711,566 seeds on empty domains and a further 376,858 by delta ownership. One separate Raytrace RSS observation was 3.97 GB for residual and 3.94 GB for entry (decimal GB, whole process). This is not a memory bound; the per-Run cache size depends on the number of distinct projected keys.

All eight matched on the final full-state export and per-Run scheduling/binding counts. Seven smaller cases matched on the initial graph and each of 9,971 Run graph, binding and schedule snapshots. An independent diagnostic matched Raytrace's ordered RHS input digest at all 16,883 Runs and its full endpoint. Intermediate full Raytrace graphs were not compared. The triangle regression exercises late insertions, forced hash collisions, old-artifact fallback, emitted plans and rejection of corrupted plan wiring. [Raw timing samples and counters](entry-results.json)

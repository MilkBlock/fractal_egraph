# Hierarchical histogram prefix sums

The resident radix sorter previously used only 16 threads to scan all histogram
blocks, one thread per digit. The new path scans 256-block tiles in parallel with
SIMD prefix sums, scans only the tile totals, and adds the tile offset during the
existing scatter. Stable ordering and exact integer counts are retained. The
additional scratch is proportional to histogram tiles, not rows times plans.

The 40,000-seed index fixture crosses the 65,536-entry tile boundary and compares
all native facts plus exact reference rows/IDs/history. Its single paired times
were 0.915284 -> 0.889656 s; this small fixture does not establish a large gain.

The complete real Pass 3 with the same preceding options completed in
**201.743776 s**, compared with 209.981214 s before hierarchical scans. There are
still 9,782 logical Runs. The requested 6.17-second target remains unmet.

A 32-Run diagnostic from checkpoint 2,000 verifies all state/history against the
same frozen worker. Profiling times are diagnostic, not full-program shares.
See `/Volumes/Extreme SSD/eggfuse-profile/hier-pass3-warm-phases/timings.json`.

# Exact active-row compaction and single-step Cell no-ops

The resident matcher now unions the actual per-rule delta requirements by seed
table. It computes fresh/old timestamp bounds once, scans rows in parallel and
packs only possible starting rows into a shared row-coordinate vector. Full
queries receive the original physical row index. Grounded variants use a
separate synthetic lane; scalar queries keep their original path. No candidate
budget, sampled absence, or reduced Bake library is involved.

An important regression starts with an old Pair row, then updates a completely
keyed function that makes that old Pair newly match. This tests the `old` and
`all` routes, rather than incorrectly retaining only newly timestamped seed rows.
Grounded zero-atom initialization and allocating Set query primitives are also
checked against native execution.

Single-step Cell recipes can use the existing stable RHS no-op proof in parallel.
Multi-step recipes are deliberately excluded: a no-op set can still expose a
changed actual value to a descendant. Skipped seed tickets are marked done once,
so an unrelated union continuation cannot count or execute them again.

Real complete Raytrace Pass 3 (`seeded-pass3-full`):

- 9,782 Runs; 701,998 stored rows; same final rows, values and equality state as
  the preceding full run.
- 3,565,719,944 physical-row examinations, **4,212,412 active seed rows** (0.1181%).
- 2,332,169 single-step Cell no-op checks, **1,009,429 successful proofs** (43.28%).
- Inference **209.981214 s**, versus 214.363695 s before these changes.

The small net gain despite much less matching work is evidence that these are
not sufficient to reach the historical 12.34 -> 6.17 s target. That target is
still unmet. These counts are starting-row jobs and no-op proofs, not committed
mutations or an end-to-end Cell-only speedup.

Sources: `check_seeds.py`, `check_cell_noops.py`, `pass3-completed.json`. The
ordinary 19 fixtures, eight private-schedule fixtures and four checkpoint cases
passed with compaction enabled.

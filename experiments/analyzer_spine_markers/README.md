# Shared matching spines in the analyzed-Cell compiler

This change accelerates **offline projection of native query fragments onto
analyzer-produced closure certificates**. It is connected to the existing
`cell_prefix::Library -> family_program -> table_live` path. It does not modify
native tracing/ripen, the Metal kernels, or the first live port read. In particular,
it does not eliminate the runtime Get port reads reported in the parent experiment.

Previously, every query/anchor/prefix called the symbolic matcher again, scanning
each closure's rows. Equal fragments were merged only after that work. Now:

1. Normalize each ordered spine by operator, column type, constant and variable
   wiring. Rule names, variable spelling and unrelated residual guards are absent
   from this key; repeated-variable constraints are retained.
2. Intern a marker for each prefix. Keep its correlated symbolic bindings and
   owner Cell IDs. A longer spine extends the existing prefix; another consumer
   restores its own slot numbering from the same marker.
3. Index closure rows once. After the anchor, only fully keyed reads are supported,
   so extending a prefix probes those keys instead of comparing every binding
   against every row. All rows for a key are retained. Root/non-root positions are
   treated alike; hidden rows and RipenInput constructors are excluded.

Markers belong to an immutable loaded library. A changed library requires a new
index. A marker is **not** a live saturation or absence certificate. Existing
generation checks, missing-key watches, fallback queries and numerical guards
remain unchanged. Empty projections give no Cell credit.

## Validation

From the repository root:

```sh
cargo test --offline --manifest-path cell-assisted-egglog/Cargo.toml --lib spine_tests -- --nocapture
```

Three tests compare every result with the previous symbolic matcher and verify
that every generated GPU program field except the new diagnostics is identical.
They cover alpha renaming, constants, aliases, type separation, hidden rows,
multiple Cell owners, non-root positions and correlated bindings. Existing library
unit tests also pass (13 tests total).

Measured deterministic work, **not a timing or runtime GPU speedup**:

| Workload | Result |
|---|---|
| Five native consumer rules over the checked-in analyzed fixture | 20 projection requests; 11 prefix markers; 30 prefix reuses; 11 binding/row comparisons |
| Synthetic 64-position symbolic closures, three ordered spine projections | 32,651 old binding/row comparisons -> 639 indexed comparisons; same answers |

The synthetic result measures the combination of keyed lookup and shared prefixes;
it is not a Cell semantic credit or an eggcc speedup. A repeat request for an
alpha-equivalent spine performs zero new row comparisons, but still transports
and deduplicates the returned binding conditions.

Set `CELL_SPINE_MARKERS=0` before loading the library to use the old projection
matcher. Diagnostics appear at `cells.spine_projection` in the runtime report,
separate from live Cell/search credit. Index construction is currently performed
in either mode, so this switch isolates matching work, not total loading cost.

## Cost and limit

Let R be closure rows, U distinct requested prefixes, W_p retained bindings at
prefix p, and B_p its slot count. Rows are stored once in the new index; directories
reference row IDs. A new anchor scans only its operator's rows. A suffix performs
key lookups for its parent's bindings; reusing a prefix avoids those lookups.
Index memory is O(R * row width); retained witnesses use O(sum_p W_p * B_p).
There is no bound on the number of genuinely distinct spines, no sampling and no
claim of constant memory. General reverse joins still use the existing fallback.

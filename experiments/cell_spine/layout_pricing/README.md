# Source reuse and per-transition layout projection

This optimizes the real Python Cell compiler's structural cost construction.
It does not change egglog's execution engine, ripen rules, available canonical
spines or the finite layout domain. It introduces no candidate budget or
approximate MIP termination.

## The two changes

1. Compile the sources of a transition once. Retain known bindings, certified
   subtree values, actual canonical-row existence, owners and pending decoders.
   Resume each target from copied mutable collections. Keep decoder obligations
   until the target actions reveal which variables are used. Cache the final
   LHS variable-alias/query normalization under exact AST equality. A borrowed
   target may add lookups; those enter the cache key too.
2. Project each layout to the fields this transition actually uses. A source
   requires its read recipe; a new marker requires its creation recipe; retiring
   a marker requires its key. A family read and written by the same transition
   retains both recipes. Different bindings, guards and physical-table modes
   remain distinguishable. Identical projections point all their original
   variable labels to one cost branch. Marker-column storage still uses the
   original layout variables, so projection cannot secretly alter storage cost.

The lookup normalizer also avoids generating alpha-renamed copies of complete
rules when only the normalized AST is requested. Takeover's alpha-equivalence
keys remain unchanged. A one-time comparison with revision `85acc59` checked
identical ASTs for 1,360 small layout assignments. The permanent exhaustive oracle compares all new pricing modes
against uncached lowering, including compound bindings, union guards, multiple
roots and raw/canonical storage.

Caches live within a profile/source tuple. They do not retain every source AST
for the whole catalog. Target-dependent aliasing and decoder decisions remain
part of lowering, not guessed from a Cell ID. The normalizer cache is tied to
the same rule/signature environment.
Its peak storage is proportional to the distinct final LHS bodies for one source
tuple, in addition to the existing cost diagram; this can still grow with target
choices. There is no hidden cache eviction or candidate cap.

## Reproduce

Use the native tools and Python environment described in
[`tools/cell_spine/README.md`](../../../tools/cell_spine/README.md). Capture the
If source once, with windows explicitly enabled:

```sh
EGG_LAYOUT_CELL_WINDOWS=1 EGG_LAYOUT_GRAPHS=0 RAYON_NUM_THREADS=1 \
  target/release/egg_layout analyze \
  --source experiments/cell_spine/eggcc_if_tiny/source.egg \
  --output out/layout-if-analysis

out/cell-spine-venv/bin/python experiments/cell_spine/layout_pricing/run.py \
  --analysis-dir out/layout-if-analysis --output out/layout-if-ablation
```

The script runs `enumerate`, `source-cache`, and `projected` once each and keeps
logs, exact solver artifacts, native validation and a small comparison report.
All output directories must be new. To run only the new path, pass
`--modes projected`. To measure a smaller program, pass `--source` and its matching
analysis catalog. All runs use the same catalog and SCIP backend. The reference
enumerator uses the current normalizer too, so this ablation isolates caching
and projection from that common normalization improvement.

## Measured If ablation (2026-10-13)

Same native analysis catalog, one quiet sequential run per mode, actual compiler
and native validation. No build or fresh tier-0 capture in these times. Full
versions, objective, counts and solver timers are in [`results.json`](results.json).

| Mode | Compiler time | Layout profile | Layout evaluations | Source compilations | LHS normalizations |
|---|---:|---:|---:|---:|---:|
| Complete enumeration | 163.86 s | 158.21 s | 107,668 | 107,668 | 107,668 |
| Source + LHS normalization cache | 126.24 s | 121.06 s | 107,668 | 38,315 | 38,315 |
| Both, including transition projection | 124.13 s | 118.84 s | 105,976 | 38,315 | 38,315 |

All three obtained objective **2830**, retained **87 physical rows** before
expansion, and passed **11 full native closure checks**. The same 232 original
layout variables remain selectable. Source-first cost-diagram ordering yields
365 variables / 623 constraints for each marker model in this experiment.
The generated `library.egg`, `runtime.egg`, and `restore.egg` are byte-for-byte
identical across these three runs; their SHA-256 digests are recorded too.

Total compiler time falls about **24%** against the current uncached enumerator.
Projection alone avoids 1,692 evaluations (about 1.6%); the main benefit is cached
source/normalization work. The earlier pre-refactor result was 187.55 s with
184.31 s in profiling; that is historical context, not this controlled ablation.
The common normalizer improvement benefits every mode above.

The remaining 118.84 s still constructs and normalizes target actions and prices
all distinct projected tuples. This is not a claim that the Cartesian-product
problem is solved, or that the generated egglog runtime itself is faster.

## Complexity and limits

For a transition, let `L_f` be each family's layouts and `K_rf` its distinct
transition projections. Profile leaf evaluations change from `product L_f`
to `product K_rf`. No guarantee says these projections will be much smaller.
Source compilation occurs once per projected **source** tuple; continuation
still occurs per full projected tuple. Equal resulting LHS bodies reuse their
normalization plan. Cost diagrams continue sharing equal suffixes afterwards.

The worst case is still exponential in jointly interacting families. Rules
reading every involved family cannot save source construction across an
independent target family. Source caching does not make native instance-storage
measurement symbolic; instance mode still evaluates the exact selected original
layouts. Neither construction time nor a small cost diagram is a runtime Join
speed claim.

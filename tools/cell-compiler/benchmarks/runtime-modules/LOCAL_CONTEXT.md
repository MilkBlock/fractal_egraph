# Root-keyed query contexts in the standalone Cell CPU executor

The historical Raytrace experiment traced a five-atom `always-run` rule. It paired a loop with every constant in its output context and constructed the corresponding constant in its input context for `lsr-inv`. The earlier CPU/GPU matcher spent 31,846,434 GPU candidate visits on that query. A CPU boundary program maintained 84 loop roots, checked 126,320 rows and returned the same 123,254 native bindings. The one reported full inference sample changed from 13.387 to 12.456 s. Those measurements belong to the historical hybrid runtime. [Historical experiment and source rule](https://github.com/MilkBlock/eggfuse/blob/86c1a98/experiments/pass3_boundary_programs/README.md)

## What the compiler can prove

From the typed seminaive LHS the Rust compiler looks for a unique constructor root with at least two distinct input slots. Every intermediate table read must share a key slot with the root, collectively covering at least two root keys. The terminal shares no slots with the root directly. Its key contains both a slot supplied by the prefix and a slot still to be enumerated. All atoms must be table reads. The artifact records the root, port order, terminal join columns, equality tests, and source-ordered RHS input projection. The loader checks these against the recipe and derives them for older artifacts.

Both frozen artifacts have one eligible rule. For rule 249, the compiler chooses the following order without consulting table names, rule IDs, or training examples.

```text
root    DoWhile(inputs, body) = loop
ports   ContextOf(inputs, input_ctx)
        ContextOf(body, output_ctx)
        HasArgType(inputs, in_type)
tail    Const(c, out_type, output_ctx) = constant
RHS     lsr-inv(loop, Const(c, in_type, input_ctx), constant)
```

There can be multiple values for a port and multiple loops for a terminal key. The engine keeps complete correlated prefix tuples per root and groups them by the terminal's shared columns. Each root holds read leases on index buckets, including missing buckets. After prefix tables change, the engine checks root epochs and leases, rebuilding affected roots. Visible insertions, replacements, removals and canonicalization participate in ordinary index updates. The executor admits at most 256 roots, requires the root table to be no larger than the terminal, limits prefix rows plus leases to 4,096, and declines port buckets larger than 4,096. If the context grows beyond budget, the ordinary plan supplies the entire answer. A later prefix change retries admission.

At the scheduled Run, a new prefix must still see older terminal rows. A stable prefix needs only the shared scheduler's terminal delta. Sorting each group's prefixes by epoch lets an old terminal row visit just the fresh suffix. The executor orders and deduplicates the resulting full bindings as usual. It retains no terminal answers. Rules with a certified stage-Spine entry stay eligible for their existing block switches. A prior block execution cannot prove that the current fan-out under a partially bound terminal key is complete. This experiment reuses the typed prefix while the frozen iterative catalog continues to handle certified handoffs.

## Measured scope

| Workload | Rule candidates (off → on) | Rule search, ms | Pass candidates | Pass inference, ms | Faster inference pairs |
|---|---:|---:|---:|---:|---:|
| sum-bits | 7 → 14 | 0.034 → 0.025 | 19,221 → 19,228 | 34.550 → 34.157 | 5/7 |
| raytrace | 381,729 → 152,218 | 149.313 → 50.455 | 8,647,393 → 8,417,882 | 13,837.284 → 13,843.621 | 2/4 |

The Raytrace rule emits 143,282 logical bindings in either arm. In a separate baseline profile it ranked tenth by search time. The first three rules maintain `Expr-size` for `Get`, `Single` and `Concat` and together spent 1.85 s in search. Turning on the context removes 229,511 candidate visits from the full workload (2.65%). The median paired inference change is +7.157 ms on Raytrace and −0.195 ms on sum-bits. Raytrace's measured pairs change direction. These runs establish a rule-level improvement on Raytrace, while the full-pass timings support keeping the option off by default. Raytrace CLI medians were 16.519 and 16.601 s. Sum-bits CLI medians were 134.471 and 131.913 ms.

The per-rule candidate counter includes decoded roots, port candidates, terminal row examinations, and every terminal/prefix pair tested. Per-root metadata and bucket-lease checks are maintenance. Raytrace reused 10,036 roots and rebuilt 160. It issued 395 port probes covering 339 rows, inspected 7,889 full-domain and 548 delta terminal rows, and tested 143,282 pairs. The engine retained at most 84 roots and 84 correlated prefixes. An exploratory implementation without the epoch-sorted suffix tested 528,579 terminal/prefix pairs on the same input. Sum-bits retained a single root and prefix.

Both configurations used the frozen 64-block artifacts with the guarded RHS handoffs, one thread, ordinary storage, shared nodes, planned Free Join, Sieve and column intersection. The comparison changes only `CELL_CPU_LOCAL_CONTEXT`. Measured pairs exclude one warm-up per arm. Inference excludes loading, offline training, export and diagnostics. CLI includes loading, inference and final export. Rule times come from separate profiled pairs. All timed runs verify the schedule (including epochs and per-Run binding counts), final graph, resumptions, maximum depth, logical bindings and committed operation counts. Separate audit builds compare the ordered RHS digest for each Run byte-for-byte and the entire winning Spine prefix tree against the frozen references. [Raw paired samples, counters and hashes](local-context-results.json)

## Reproduction

Use the `fixed.json` for each workload from the [RHS completion experiment](RHS_COMPLETION.md#reproduction). Run from the worktree root and choose fresh output directories. For the real-workload ablation use the runtime Cargo invocation so both arms retain the same iterative catalog.

```sh
m=engine/cell-runtime/Cargo.toml
export CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1 CELL_CPU_FREE_JOIN=planned
export CELL_CPU_SHARED_NODE_SCHEDULE=1 CELL_CPU_SIEVE=1 CELL_CPU_JOIN_INTERSECTION=1
export CELL_CPU_KNOWN_RESULTS=static CELL_CPU_RANGE_GUARDS=1 CELL_CPU_SLOT_TRANSFER=off
export CELL_CPU_FD_KEYS=0 CELL_CPU_SCALAR_MERGE=0
for on in 0 1; do
  CELL_CPU_LOCAL_CONTEXT=$on cargo run --release --locked --manifest-path "$m" \
    --features iterative-bake -- --program "$fixed64" --output "timed-$on"
  CELL_CPU_LOCAL_CONTEXT=$on cargo run --release --locked --manifest-path "$m" \
    --features iterative-bake,spine-shapes,slot-transfer-audit -- \
    --program "$fixed64" --output "checked-$on"
done
cmp checked-0/rhs.json checked-1/rhs.json
```

To inspect per-rule timing set `CELL_CPU_PROFILE=1` for a separate run. For new sources, `cargo cell-compile --source input.egg --emit-free-join-entry --output compiled` emits the checked plan. A small program can also be compared through `cargo cell-bench --program compiled/program.json --lane free-join-planned --modules local-context --repeat 5 --verify-stages`.

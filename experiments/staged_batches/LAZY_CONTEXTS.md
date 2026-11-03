# Reclaim resident contexts only under allocation pressure

The real resident ABI7 engine still checks read-table versions before reuse. It
now retains obsolete blocks until an allocation would fail, then compacts live
blocks and retries the untouched Run. It grows only when the live data itself
needs more space. RHS effects are never replayed to recover capacity. Completion
certificate buckets are rebuilt only when capacity, union or visibility changes.

Reproduce (one measurement per variant, not a full Pass 3 benchmark):

```sh
python3 experiments/staged_batches/check.py --build out/eggfuse-clean-paths-build \
  --output out/lazy-context-forced --lazy-context-comparison
```

The fixture updates a correlated three-read spine 62 times while retaining an
unrelated larger context. All rows, identities, values, visibility, watermarks
and 126 per-Run fingerprints agree with eager maintenance.

| Metric | Eager | Lazy |
|---|---:|---:|
| Compactions | 66 | 4 |
| Cache capacity, i64 words | 69,120 | 69,120 |
| Inference seconds | 0.123533 | 0.123255 |
| GPU command-buffer seconds | 0.103680 | 0.100708 |

This removes maintenance work but demonstrates **no material end-to-end speedup**
on this small fixture. Full Pass 3 remains a separate acceptance test.

Also passed: all 19 staged-batch fixtures (including expected overflow), four
checkpoint cases, and 30 compiler/IR/reproducibility unit tests. Batch layout v4
adds index-generation metadata; v3 artifacts remain loadable with their own
previous shader and layout.

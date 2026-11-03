# Speculative key memory, stored as dense response weights

The GPU does not have to reconstruct an exact dictionary lookup. This prototype
stores a function which suggests predicate responses; CPU validation and
complement/fallback remain the authority. It is a standalone frozen-snapshot
experiment, not a modified egglog execution engine or a complete fallback loop.

## Storage and computation

A key is a tuple of typed canonical IDs/literals. Its hash is computed directly
from that tuple, without asking whether it exists in a target table. A fixed
generator maps it to 256 signed coordinates `r(k) in {-1,+1}^256`. GPU input
contains only CURRENT probe fingerprints, not a dictionary of all stored keys.

Group compatible key schemas, such as one Expr identity, and keep multiple
predicate channels in the same block. The real Expr group contains 43 channels.
A cheap hash selects a shard; all requests for that shard share one matrix:

```
GPU resident: prior beta[C], FP16 weight matrix W[256,C]
query tile:   R[queries,256]
response:     S = beta + R W / 256
```

Each row of S answers several conditions together. Thresholding scores gives
speculative memberships, from which the original conjunctions form candidate
rule/anchor indices. Exact literals/aliases in the anchor and scalar empty-key
facts retain their earlier checks. No GPU hash-table lookup supplies memberships.
Original bindings and whole-row payloads remain on the CPU.

Two encodings are implemented and BOTH retained in the report:

1. **Additive memory**: `W = R_bake^T (Y-beta)`. This is cheap to update but
   different keys interfere. Even after removing the majority response, this
   version loses to the constant-prior-only quality control in this dataset.
2. **Bake-corrected memory**:
   `W = 256 R_bake^T (R_bake R_bake^T + 0.001 I)^(-1) (Y-beta)`.
   This calibrates the decoder on the actual known key responses. There are
   fewer baked keys per shard than coordinates. Every original key is retained
   in the fit; sharding enforces dimensional capacity, not a candidate budget.

Y is built from the observed native fact tables, not query-match labels. Unary
domains include IDs from ALL visible raw facts of that sort. Compound domains
are the union of actual projected table keys. The approximate model does not
know that an unseen tuple is absent. This is deliberately a proposal mechanism.

For the corrected version the shard count is a power of two, large enough for
an average load at most 3/4 of the feature width; the actual maximum load is then
checked to be below the width. This rank margin is an explicit physical layout
choice, not an optimized universal constant. Overloaded shards split; no key is
truncated. All known-key classifications are checked again after FP16 storage.
There is no theorem guaranteeing arbitrary future-key accuracy.

The GPU worker removes coefficient columns that are identically zero, stores
their prior only, and packs the remaining columns in contiguous 8-column tiles.
It executes exactly three kernels:

1. Generate dense signed coordinates from current probe hashes.
2. Fill output channels with their prior response.
3. Execute all nonconstant blocks in ONE grouped `simdgroup_multiply_accumulate`
   kernel, scatter active channels, and add the prior.

This avoids launching one GEMM for each of the 1,373 request/shard groups and
does not construct an enormous block-diagonal matrix containing unused products.
The implementation uses Apple's matrix instructions. It is not an NVIDIA
Tensor Core measurement and does not establish 50% of neural-network throughput.

## What was actually verified

This uses the SAME frozen Pass-3 input and anchor-complete class from
`../query_response_operator`, with the complete native-verified root projection
digest checked before evaluation. That class has 452 eligible source queries,
282 with a nonempty anchor table. It is not all Pass-3 rules or all runtime work.

The memory addresses 20 key schemas and 75,035 distinct queried keys. The
underlying key domains contain 184,131 keys. For the 256-coordinate corrected
version, its 1,031,878 condition scores imply:

| Predicate memory | Candidate precision | Recall of true rule/anchor pairs |
|---|---:|---:|
| Additive, 256 coordinates / 8 shards | 86.26% | 82.04% |
| Constant prior alone, same partition | 97.29% | 86.42% |
| Corrected, capacity-based shards | **99.00%** | **100%** |

The last row proposes 562,411 root/rule pairs: 556,797 are true and 5,614 are
false positives. Threshold is 0.5. The full report also retains all declared
width/shard configurations and thresholds, including worse results.

**This is known-key calibration, not generalization.** A separate experiment
withholds 23,227 keys (roughly one eighth) from Bake. On rule/root queries that
actually read a withheld key, precision is **81.01%**, recall **65.05%** and
14,020 true queries are missed. Constant prior alone achieves 92.58% precision
and 66.65% recall there. Arbitrary hashed IDs supply no semantic generalization
to unseen bindings. Overall recall of 97.48% in that experiment would hide this
problem, so the withheld-dependent subset is reported separately.

Those missed-positive counts are measured with an oracle. Runtime cannot obtain
the missing list for free: it must maintain and execute a complement/fallback
process over remaining queries. Merely validating GPU-positive candidates does
NOT establish completeness. This prototype has not integrated that process.

## GPU result and complete memory accounting

For the corrected 256-wide bank, one warmup plus one retained M3 Max execution:

* 663 nonconstant matrix blocks, processed in one grouped matrix kernel.
* 38,664 probe keys need matrix evaluation; constant-only blocks use priors.
* 314,341,376 useful matrix FLOPs; 1,031,878 scores returned.
* **0.5105 ms device time**, including coordinate generation and prior/scatter.
* 0.7695 ms host encode/submit/wait.
* Maximum score difference from CPU FP16-weight reference: `1.19e-7`.
  **Zero 0.5-threshold disagreements.**

Storage is NOT just the weight file:

| Allocation | Bytes |
|---|---:|
| Packed weights, priors and routing descriptors | 4,428,056 |
| Current nonconstant probe hashes | 309,312 |
| Temporary dense query coordinates | 19,795,968 |
| Output scores | 4,127,512 |

The unpruned corrected bank contains 5,661,036 coefficient/prior bytes. These
are buffer sizes, not process RSS, total CPU storage or a proven compression
ratio versus a well-designed exact index. Native facts still exist on the CPU.
The worker consumes pre-bucketed requests; their CPU hashing/grouping, Bake,
validation, complement work and RHS commitment are excluded from device time.
There is no end-to-end speedup claim or exact GPU hash-index timing control here.

The initial additive form uses only 478,848 coefficient/prior bytes at width
256 / 8 shards, but the poor quality and the stronger prior-only baseline prevent
calling that a successful small-memory replacement.

## Updates and current limits

For additive memory, a net logical membership change admits the outer-product
update `delta W = r(key)^T delta payload`, and multiple updates can be batched
as a matrix product. Duplicate witnesses must not be added twice: the CPU must
supply first-witness/last-witness membership changes.

**That additive update is invalid for the corrected decoder.** The implementation
explicitly rejects it. A corrected block needs a refit or an appropriately
maintained inverse/factorization. Canonicalization can change keys; retractions,
new domain keys, table visibility and epochs must be coordinated with the native
runtime. None is silently considered solved by the frozen test.

The most consequential limitation is that this bank is calibrated to current
canonical identities. Reusing weights across new Cell instances would require
better, structural/relative coordinates from Analyze, or explicit block updates.
The experiment demonstrates **key information stored in dense coefficients**,
not a general semantic Cell embedding, not a proof of irrelevance, and not a
replacement for the native effect protocol.

## Reproduce

Use Python with NumPy. `NATIVE_REPORT` is the unchanged complete report from
`query_response_operator` (must contain `root_projection_sha256`).

```sh
python -m unittest discover -s experiments/superposed_key_memory -p 'test_*.py' -v
python experiments/superposed_key_memory/run.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --native-report "$NATIVE_REPORT" --output out/key-memory
python experiments/superposed_key_memory/holdout.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --output out/key-memory/holdout.json
/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc -O -sdk /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk experiments/superposed_key_memory/gpu.swift -o out/key-memory/worker
out/key-memory/worker out/key-memory/input.bin out/key-memory/reference.bin out/key-memory/gpu.json
```

Generated packets and binaries are untracked. `results.json` records the retained
quality experiments, native input identity, held-out test and GPU results.

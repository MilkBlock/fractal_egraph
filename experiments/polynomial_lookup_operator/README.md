# Searching for a useful dense representation of arbitrary eggcc queries

**Result: not an acceleration.** An exact finite-field operator turns generic
partial lookups into two dense MPS GEMMs, including a conservative membership
decision and a payload handle. On one real saved Pass-3 request bank it reaches
51.2% of a same-process dense-GEMM throughput reference, yet takes **25.4 times
as long as a GPU hash/bitmap implementation of the same filter**. This rejects
the idea that merely replacing table access with highly occupied arithmetic is
enough. There is no production runtime change or NVIDIA Tensor Core measurement.

These are arbitrary raw typed IDs and compound integer keys, not selected
parameterized Cell recurrences, learned match labels, or hand-picked scalar
functions. Physical pages below are NOT RipSatCells.

## Representation and exactness

For each context of at most 127 projected keys, hash raw 64-bit canonical IDs
and exact i64 bit patterns into two independent coordinates in `F_65521`.
Keep all colliding original keys and row payloads. Build:

```
P1(z) = product(z - first_hash(key))   over distinct first hashes
P2(z) = product(z - second_hash(key))  over distinct second hashes
F1(first_hash(key)) = first-hash bucket handle
```

The runtime evaluates these polynomials for many queries together. A query
passes when both P values are zero. F1 then returns a bucket; exact original-key
validation returns ALL matching row indices. The two zero tests can be
witnessed by different original keys, so false positives are allowed. True
keys cannot be missed under the exact arithmetic used here.

This is a **candidate page/bucket test**, not a complete rule match. Remaining
atoms, guards, visibility, Run watermarks and RHS effects are not executed.
Tables are the saved canonical visible snapshot; live union/rebuild invalidation
has not been connected to this experiment.

Coefficients and powers are ordinary non-Boolean field elements. Center each
element and split it into two exactly representable FP16 digits:

```
x = low + 256 high,   |low|, |high| <= 128.
```

Four FP16-input / FP32-accumulated products reconstruct a field product:

```
C = (C00 + 256(C01+C10) + 15 C11) mod 65521.
```

The `15` is `256² mod 65521`. Each dot has width at most 512 and absolute
intermediate integer sum at most `512*128² = 2²³`, so the accumulation is exact
in FP32; modular reconstruction uses exact integer operations. Every returned
GPU handle is also checked against an independent hash implementation.

Stacking `[A_low; A_high] [B_low | B_high]` computes all four needed digit
products in one GEMM. All quadrants are used. First-hash powers are shared by
P1 and F1; second-hash powers serve P2. Thus the complete bank uses **two GEMMs
per query tile**, followed by elementwise modular reconstruction and zero tests.
This reduces launches without padding the rule set with unused interactions.
The final partial query tile does contain ordinary alignment padding; reported
useful FLOPs exclude those padded query columns.

Coefficient updates also have an exact composition law, tested on CPU:

```
P_new = (z-q) P_old
F_new = F_old + (handle-F_old(q))/P_old(q) * P_old.
```

Removal uses synthetic division and polynomial remainder. These operations
apply to distinct hash roots; the caller must maintain original-key bucket
multiplicity/refcounts and a consistent epoch. GPU update journals are not
implemented. Fewer coefficient bytes do NOT establish lower RSS.

## Real input and controls

`prepare.py` consumes the existing 609-query native plan header and saved
Pass-3 facts. There are also 55 rejected query shapes in that header. For the
chosen table/projection it includes every admitted seed plan whose first next
atom reads that table. It reads **only the seed atom**, applies its literal and
local-alias constraints, and forms the next partial key. It does not search the
target table, evaluate guards, or precompute full matches to build the inputs.
Alternative seeds are included and keys are deduplicated across consumers.
This is not the actual stream of per-Run requests.

An initial discarded prototype encoded a field using target-table dictionary
ordinals. That can already answer unary membership during preprocessing. Its
results are deliberately excluded. All recorded results here hash **raw IDs
and i64 bits**; a regression verifies that a key's encoding does not depend on
the other observed keys.

| Raw-key bank | TCPair output class | Get parent + index |
|---|---:|---:|
| Source query shapes contributing requests | 4 | 2 |
| Distinct partial requests | 63,038 | 8,824 |
| Target rows | 63,038 | 13,341 |
| Physical pages | 497 | 106 |
| Returned candidate pages | 63,066 | 4,440 |
| False candidate pages | 28 | 4 |
| Exact recovered row indices | 63,038 | 4,436 |
| Missing matching row indices | **0** | **0** |
| Polynomial GPU time | **18.677 ms** | **2.278 ms** |
| Same-filter GPU hash time | **0.736 ms** | **0.120 ms** |
| Polynomial / hash time | **25.36×** | **18.91×** |

TCPair uses stacked GEMMs and query tile 8192; Get uses tile 2048. Each is one
run, not an averaged or best-of timing. The TCPair bank performs **96.25 billion
FLOPs**; its 5.153 TF/s is 51.2% of the paired 1024³ MPS reference's 10.064 TF/s.
This is neither theoretical peak utilization nor a NVIDIA Tensor Core result.
Clock/performance variation across the individual runs prevents attributing
all cross-run differences to layout. `results.json` retains both losing 2048
layouts as well, not just the wider tile.

Device time includes power generation, GEMMs, reconstruction and masking.
It excludes source loading, coefficient construction, compilation, and CPU
payload verification. Host submit/wait is recorded separately. The hash control
performs the SAME all-page filter and returns identical handles. A global
exact-key index could avoid even that all-page work, making this a generous
control for the matrix method. No inference/extraction speedup is claimed.

## A second check: is the full rule bank actually dense?

`factor_audit.py` projects each atom onto the variables shared with other atoms,
retains all original rows as correlated payloads, and shares identical typed,
literal- and alias-constrained projections across queries. Local variables are
not semantically erased: guards and effects can still read them from payloads.

For all admitted query shapes, the audit finds:

* 1,845 atom occurrences reduce to **437 distinct projected factors**; 248 are
  shared by multiple queries. This is structural sharing, not a measured search
  reduction; the engine already shares raw tables and indexes.
* **515/609** query shapes need only scalar, unary or binary projected factors.
  Of these, 498 have a forest atom/variable graph before primitive guards.
  Binary factors do not imply path queries, low rank, or complete matching by
  pairwise tests. Tests retain cyclic and correlated-key counterexamples.
* The 58 nonempty binary factors have 525,287 entries in rectangular carriers
  totaling 7,012,972,288 coordinates: **0.00749% occupied**.
* Allocating only nonempty 32×32 tiles still uses 108,499,968 slots: **0.484%**
  occupied. These are individually sorted compact axes, not an optimized global
  layout or a lower bound against all possible coordinate systems.

Examples include HasType shared by 70 query occurrences and lo-/hi-bound each
by 30. This supports looking for shared query structure beyond special Cell
families, but **does not justify directly turning these factor tables into
dense GEMMs**. The missing step is finding a representation whose dense work
answers many jointly needed conditions, instead of expanding sparse identities
or multiplying the same lookup work by a polynomial width.

## Reproduce without rerunning eggcc

Use Python with NumPy. Set `SNAPSHOT` to the saved `cpu.control.json` and
`PROFILE` to the native `pass-3.jsonl.gz`; their SHA-256 values and source paths
are in `results.json`. Choose fresh output directories.

```sh
python -m unittest discover -s experiments/polynomial_lookup_operator -p 'test_*.py' -v
python experiments/polynomial_lookup_operator/prepare.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --table TCPair --mask 4 --output out/poly-tcpair
python experiments/polynomial_lookup_operator/prepare.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --table Get --mask 3 --output out/poly-get

/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc -O -sdk /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk experiments/polynomial_lookup_operator/gpu.swift -o out/poly-gpu
out/poly-gpu out/poly-tcpair/input.bin out/poly-tcpair/gpu --stacked --batch 8192
out/poly-gpu out/poly-get/input.bin out/poly-get/gpu --stacked
python experiments/polynomial_lookup_operator/verify.py --data out/poly-tcpair --prefix out/poly-tcpair/gpu
python experiments/polynomial_lookup_operator/verify.py --data out/poly-get --prefix out/poly-get/gpu
python experiments/polynomial_lookup_operator/factor_audit.py --snapshot "$SNAPSHOT" --profile "$PROFILE" --output out/poly-factor-audit.json
```

`field.py` is the exact reference algebra; `gpu.swift` is the standalone MPS
operator. The `.bin` packets and compiled workers stay untracked. The source
rules and engine are unchanged.

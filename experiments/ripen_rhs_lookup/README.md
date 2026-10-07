# Preserve existing RHS constructor lookups as replay dependencies

A constructor used by a rule's RHS can already exist with a canonical output
established by earlier unions. Previously `LookupOrInsertDefault` returned that
value without recording a dependency: no new insertion meant no write receipt.
The replay could reconstruct its syntax but miss the existing equality.

The dependency tracer now optionally records successful RHS lookup rows, their
source spans and available producer provenance before evaluating missing rows.
These are explicitly tagged `rhs_lookup`; they are reads, not committed writes,
and do not change LHS witness completeness. Native analyze capture opts in;
ordinary dependency tracing and non-traced execution retain their old default.
The existing entry extractor can then use those rows as actual input dependencies,
including canonical constructor outputs, rather than inventing an alias union
from a failed postcondition.

Old saved histories cannot recover the missing evidence and must be recaptured.
The change does not add helper saturation before checks or suppress alias checks.
It also does not claim to capture every RHS primitive or mutable lookup.

Regression: normalize a Cat constructor in an earlier ruleset, then read it
under Wrap in a later RHS. The local entry now replays with the previously missing
boundary equality. Existing set, alias and stage-order tests remain enabled.

```sh
cargo test --offline --release --lib native_analyze::ripen::entry::tests
RUST_MIN_STACK=16777216 RIPSAT_SET_AUDIT_HISTORY=/tmp/eggcc-set-final/live/history.json RIPSAT_RHS_AUDIT_OUTPUT=/tmp/rhslookup-audit.json cargo test --offline --release --lib native_analyze::ripen::entry::tests::audit_rhs_lookup_recapture -- --ignored
```

The manual audit compares the first four HasType/Concat applications and their
recorded ancestor compositions, using the old history and a new capture of the
same source. It only validates preparation; no full ripen sweep or new overall
Cell coverage measurement is claimed. It preserves full rejection reasons.

RHS-only dependencies are checked after staged external input injection and before
the original rule action. This permits an existing outer constructor lookup to
use an externally supplied inner lookup/equality. Original LHS internal reads
remain checked before injection. No post-action alias check is removed.

## Targeted result (2026-10-07)

Four same-source HasType/Concat ancestor compositions were prepared. The saved
history accepts only record 64; new capture accepts 64 and 76. Record 76 previously
failed the TupleT/TLConcat output-alias check and now passes all checks. Records
62 and 63 still fail an earlier internal dependency check at member 51. The
full historical report propagated member-76 failures to 1,048 rejected jobs;
this targeted experiment does not claim all 1,048 are now successful.

The focused regression suite passes 19 tests. Fresh capture is necessary; the old
history intentionally continues to reject the unproved alias.

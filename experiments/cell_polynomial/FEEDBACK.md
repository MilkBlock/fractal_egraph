# Cell growth through an interval operator

2026-10-09. Run `python3 experiments/cell_polynomial/feedback.py`.
This follows EXPLORATION.md but now runs successive grafts in the SAME native
egglog instance: saturate a suffix, prepend a distinct variable, saturate again.
Only original Add associativity plus a read-only Hit reporting rule is enabled.

## Representation

For n distinct ordered leaves, use n+1 interval boundaries. U[i,j]=[i<j]
denotes that interval [i,j) has a represented sum. All its parenthesizations
share that interval e-class. Then:

* U^2[i,j] counts Add split choices i<k<j.
* U^3[i,j] counts associativity LHS bindings i<k<l<j.
* Summing these entries gives C(n+1,3) Add nodes and C(n+1,4) distinct Hit tuples.

These are exact counts, not a representation of the distinct witnesses by the
numbers alone. The accompanying endpoint schema i<k<l<j preserves the actual
bindings and maps them to their interval e-classes. Hit counts are distinct
logical bindings at closure, NOT accumulated rule firings or committed writes.

Prepending one leaf introduces a new boundary and a row r of ones:

    U_new = [0  r]
            [0  U]

Consequently:

    U_new^2 = [0  r U  ]
              [0  U^2  ]

    U_new^3 = [0  r U^2]
              [0  U^3  ]

The old blocks remain identical. The new Add-node count is C(n+1,2), and the new
Hit count is C(n+1,3). With ordered endpoints, each new row entry is directly
j or C(j,2). Matrix products are only a verification reference; the endpoint
formulas do not require constructing the dense matrices.

## Native results

| Leaves | Add e-nodes | Distinct Hit tuples | Added Add nodes | New Hit tuples |
|---:|---:|---:|---:|---:|
| 12 | 286 | 715 | - | - |
| 13 | 364 | 1001 | 78 | 286 |
| 14 | 455 | 1365 | 91 | 364 |
| 15 | 560 | 1820 | 105 | 455 |

Three separate native runs start at 3,7,12 leaves, each with three successive
prepends. All twelve stages pass. Every expected Add split and every expected
Hit tuple is checked, with table cardinalities checked before/after assertions;
therefore the Hit set is exact, not merely a positive sample. Final Math e-node
and e-class counts also agree. Additional exact algebra checks cover old sizes
2 through 24 and verify unchanged old matrix blocks after prepend.

Generated .egg files and logs are under out/cell-polynomial-feedback.
The checks use let-bound interval representatives to avoid repeatedly reparsing
large nested terms. An initial verifier using repeated inline terms was stopped
because assertion preparation was unnecessarily expensive; the final verifier
checks the same sets with shared names. No speedup figure is derived from that
change or from these tests.

## Meaning and limits

This is evidence for one PARAMETERIZED saturated family: ordered leaf sequence
plus interval/split schema. Its state description need not grow with the count
of explicit matches. It handles repeated grafts without constructing a separate
abstract state definition for each length.

It is not an automatically discovered general abstraction. The interval invariant
is supplied by this specialized model; repeated leaves, commutativity, other
arithmetic rules, arbitrary unions, guards and subsume are outside its scope.
Native execution still materializes every node and performs matching; no
factorized storage, matcher bypass or compressed extraction has been installed.
The formula is a compact descriptor of all results, not a way to materialize
those results in constant time. Code deliberately distinguishes that descriptor
from the actual egglog runtime used as its oracle.

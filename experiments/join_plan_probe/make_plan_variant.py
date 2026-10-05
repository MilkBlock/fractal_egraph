#!/usr/bin/env python3
"""Plan-change variant for the raytrace export: materialise the costs that
`Smaller`'s two TermAndCost fields resolve to, once, instead of joining
`TCPair` on its non-key output column inside each of the two comparison rules.

Usage: make_plan_variant.py IN.egg OUT.egg
"""
import pathlib
import sys

src = pathlib.Path(sys.argv[1]).read_text()
out = pathlib.Path(sys.argv[2])

A = """(rule ((= lhs (Smaller (TCPair t1 cost1) (TCPair t2 cost2)))
       (<= cost1 cost2))
      ((union lhs (TCPair t1 cost1)))
        :ruleset terms-helpers-helpers :name "(rule ((= lhs (Smaller (TCPair t1 cost1) (TCPair t2 cost2)))
       (<= cost1 cost2))
      ((union lhs (TCPair t1 cost1)))
        :ruleset terms-helpers-helpers )")"""
B = """(rule ((= lhs (Smaller (TCPair t1 cost1) (TCPair t2 cost2)))
       (> cost1 cost2))
      ((union lhs (TCPair t2 cost2)))
        :ruleset terms-helpers-helpers :name "(rule ((= lhs (Smaller (TCPair t1 cost1) (TCPair t2 cost2)))
       (> cost1 cost2))
      ((union lhs (TCPair t2 cost2)))
        :ruleset terms-helpers-helpers )")"""

NEW = """(relation __smallerCosts (TermAndCost TermAndCost i64 TermAndCost i64))
(rule ((= l (Smaller (TCPair t1 c1) (TCPair t2 c2))))
      ((__smallerCosts l (TCPair t1 c1) c1 (TCPair t2 c2) c2))
      :ruleset terms-helpers-helpers :name "build___smallerCosts")
(rule ((__smallerCosts l a c1 _b c2) (<= c1 c2))
      ((union l a)) :ruleset terms-helpers-helpers :name "smaller-le-via-costs")
(rule ((__smallerCosts l _a c1 b c2) (> c1 c2))
      ((union l b)) :ruleset terms-helpers-helpers :name "smaller-gt-via-costs")"""

assert src.count(A) == 1 and src.count(B) == 1, (src.count(A), src.count(B))
out.write_text(src.replace(A, NEW, 1).replace(B, "", 1))
print(f"wrote {out}")

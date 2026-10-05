#!/usr/bin/env python3
"""Produce the base and hand-modified-plan variants of a generated eggcc program.

The modification replaces the three-stage boolean check
(`HasType pred (Base (BoolT))` = HasType -> Base -> BoolT) inside the 9-stage
If-typing rule with a single marker lookup, so the plan probes one table instead
of walking a three-hop chain for every candidate.
"""
import pathlib
import sys

src = pathlib.Path(sys.argv[1]).read_text()
out = pathlib.Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)

RULE = """(rule ((= lhs (If pred inputs then else))
       (HasType pred (Base (BoolT )))
       (HasType then ty)
       (HasType else ty))
      ((HasType lhs ty))
        :ruleset type-analysis :name "(rule ((= lhs (If pred inputs then else))
       (HasType pred (Base (BoolT )))
       (HasType then ty)
       (HasType else ty))
      ((HasType lhs ty))
        :ruleset type-analysis )")"""
assert src.count(RULE) == 1, f"expected exactly one If-typing rule, found {src.count(RULE)}"

MARKER = """(relation __boolPred (Expr))
(rule ((= p (HasType pred (Base (BoolT ))))) ((__boolPred pred))
      :ruleset type-analysis :name "build___boolPred")
(rule ((= lhs (If pred inputs then else)) (__boolPred pred)
       (HasType then ty) (HasType else ty))
      ((HasType lhs ty)) :ruleset type-analysis :name "if-typing-via-marker")"""

(out / 'base.egg').write_text(src)
(out / 'split.egg').write_text(src.replace(RULE, MARKER, 1))
print(f'wrote {out}/base.egg and {out}/split.egg')

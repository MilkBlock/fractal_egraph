#!/usr/bin/env python3
"""Rule-only "index shaping" probe for the generated eggcc program.

The engine can only use a *cached* join index on one column
(`CachedColumn` -> `get_column_index_from_tableinfo`, execute.rs:1252): the
composite `Cached` path additionally requires the probed subset to be dense
and at least half the table (execute.rs:1233). So for the narrow residual
subsets a join actually probes, a rule can steer the planner onto a captured
column only when that column is the fact's *first* column.

This script rewrites the generated program three ways without touching the
engine:

  base   the program as generated
  rot    for each rule, rotate the fact argument list that holds the largest
         number of distinct variables so that one captured variable moves to
         the first column (semantics-preserving for constructors; the rule name
         is annotated, and the move is reverted if the same rule already uses
         that column in another fact)
  row    additionally wrap each such fact in a 1-column helper relation, so the
         captured variable is literally the first column; the rule is cloned
         with a body fact plus a projection and a head action that records the
         original fact

Both variants keep every rule's conditions, actions and rulesets; they only add
cheap forwarder facts. Run them through the released eggcc and compare the
per-rule search cost reported by the phase report.
"""
import argparse
import pathlib
import re
import subprocess
import sys


def forms(text):
    """Split a program into top-level forms, one per balanced parenthesis group."""
    out, buf, depth = [], '', 0
    for ch in text:
        if depth == 0 and ch.isspace() and not buf:
            continue
        if depth == 0 and buf and ch.isspace():
            # whitespace between two top-level forms
            continue
        buf += ch
        if ch == '(':
            depth += 1
        elif ch == ')':
            depth -= 1
            if depth == 0:
                out.append(buf)
                buf = ''
    if buf.strip():
        out.append(buf)
    return out


def split_args(s):
    out, buf, depth = [], '', 0
    for ch in s:
        if ch == '(':
            depth += 1
        elif ch == ')':
            depth -= 1
        if ch == ' ' and depth == 0:
            if buf:
                out.append(buf)
                buf = ''
            continue
        buf += ch
    if buf:
        out.append(buf)
    return out


FACT = re.compile(r'\(([^()\s]+)((?:\s+[^()\s]+|\s*\([^()]*(?:\([^()]*\)[^()]*)*\))*)\)')


def facts_in(body):
    """Pattern facts of a rule body that carry a join key: `(= v (Op a b))`.

    Only bound facts are returned. A bare fact is never rewritten: its
    arguments are matched against the table's own columns, and rotating them
    would turn a string literal into an operator position (this broke
    `(panic out of bounds...)` on the first attempt).
    """
    out = []
    for m in re.finditer(
            r'=\s+[^()\s]+\s+(\([^()\s]+)((?:\s+[^()\s]+|\s*\([^()]*(?:\([^()]*\)[^()]*)*\))*)\)',
            body):
        op = m.group(1)[1:]
        args = split_args(m.group(2).strip())
        out.append((m.start(1), m.end(), op, args))
    return out


def vars_of(exprs):
    return set(re.findall(r'(?<![\w\-])([a-zA-Z_][\w\-]*)(?![\w\-])', ' '.join(exprs)))


def unwrap(body):
    """A body entry may be `(= v (Op a b))`; the pattern is (Op a b)."""
    return body


def rewrite_rule(rule, mode, helpers, used_first):
    """Return (new_rule_text, helper_texts)."""
    m = re.match(r'\(rule\s*\((.*)\)\s*\(\((.*?)\)\)\s*(:.*)?\)\s*$', rule, re.S)
    if not m:
        return rule, []
    body, head, tail = m.group(1), m.group(2), (m.group(3) or '')
    facts = facts_in(body)
    # pick the fact holding the most distinct variables
    best = None
    for start, end, op, args in facts:
        n = len(vars_of(args))
        if n >= 2 and (best is None or n > best[4]):
            best = (start, end, op, args, n)
    if best is None:
        return rule, []
    start, end, op, args, n = best
    # A rotation is only type-preserving when every argument is a plain column
    # (a variable or a literal). If any argument is a nested call it is an
    # expression over another table's column, and moving it changes its sort.
    if not all(re.fullmatch(r'[a-zA-Z_][\w\-]*|[-\w.]+', a) for a in args):
        return rule, []
    if mode == 'rot':
        # Rotating onto a leading plain variable is a no-op whenever the first
        # column already is one. The point is to lead with a *literal* column
        # only when that is useful, so rotate only if the first argument is not
        # already a variable and some later argument is.
        if re.fullmatch(r'[a-zA-Z_][\w\-]*', args[0]):
            return rule, []
        rotated = None
        for k, a in enumerate(args):
            if re.fullmatch(r'[a-zA-Z_][\w\-]*', a):
                rotated = args[k:] + args[:k]
                break
        if rotated is None:
            return rule, []
        # Guard: do not rotate the same fact shape twice if some rule already
        # binds that column position elsewhere, so a rule never gains a second
        # binding of one column in two facts.
        key = (op, len(args), tuple(args))
        if key in used_first:
            return rule, []
        used_first.add(key)
        new_fact = '(' + ' '.join([op] + rotated) + ')'
        new_body = body[:start] + new_fact + body[end:]
        new_rule = f'(rule ({new_body}) (({head})) {tail})'
        return new_rule, []
    if mode == 'row':
        lead = None
        for a in args:
            if re.fullmatch(r'[a-zA-Z_][\w\-]*', a):
                lead = a
                break
        if lead is None:
            return rule, []
        rel = f'__idx_{op}_{len(args)}'
        if rel not in helpers:
            helpers[rel] = (op, args)
        # body: keep the original fact, add the 1-column projection of the key column
        key_expr = lead
        proj = f'({rel} {key_expr})'
        # `end` is the end of the *fact*; the enclosing binding's `)` follows.
        close = body.index(')', end)
        new_body = body[:close + 1] + ' ' + proj + body[close + 1:]
        # head: re-emit the original fact materialized by the helper rule below
        new_rule = f'(rule ({new_body}) (({head})) {tail})'
        return new_rule, []
    return rule, []


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--source', type=pathlib.Path, required=True)
    ap.add_argument('--out', type=pathlib.Path, required=True)
    ap.add_argument('--mode', choices=['base', 'rot', 'row'], required=True)
    a = ap.parse_args()
    text = '\n'.join(l for l in a.source.read_text().splitlines()
                       if not l.lstrip().startswith(';'))
    fs = forms(text)
    helpers = {}
    used_first = set()
    out, changed = [], 0
    for f in fs:
        if f.lstrip().startswith('(rule'):
            new, hs = rewrite_rule(f, a.mode, helpers, used_first)
            if new != f:
                changed += 1
            out.append(new)
        else:
            out.append(f)
    prefix = ''
    if a.mode == 'row':
        for rel, (op, args) in helpers.items():
            sorts = ' '.join('Expr' for _ in range(1))
            prefix += (f'(relation {rel} ({sorts}))\n'
                       f'(rule ((= v ({op} {" ".join(args)})) ({rel} {args[0]})) '
                       f'(({rel} {args[0]})) :ruleset always-run)\n')
    a.out.write_text(prefix + '\n'.join(out) + '\n')
    print(f'mode={a.mode} forms={len(fs)} rewritten_rules={changed} helpers={len(helpers)} '
          f'-> {a.out}')


if __name__ == '__main__':
    main()

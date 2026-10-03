#!/usr/bin/env python3
"""Rule-level "narrow driver key" transform.

Pure `.egg` additions, no engine change:

  for a chosen relation and column (op, col) whose values repeat,
    1. declare `(relation __k_<op>_<col> (<sort-of-col>))`
    2. add a builder rule that projects that column out of every `op` row
    3. add the key atom to every rule body that binds an `op` fact with a
       variable in that column

The engine's planner picks the cover by table size (`plan_free_join`,
free_join/plan.rs) so a small key relation becomes the driver and the original
relation is probed on that column. Nothing is removed and no rule is restricted,
so the transform cannot lose a derivation: the key atom is entailed by the fact
atom it accompanies.

Usage:
  narrow_keys.py SOURCE.egg OUT.egg --spec ContextOf:1 --spec Get:1 [--all-top N]
"""
import argparse
import pathlib
import re

FACT = re.compile(r'\(([^()\s=]+)((?:\s+[^()\s]+|\s*\([^()]*(?:\([^()]*\)[^()]*)*\))*)\)')


def split_args(s):
    out, buf, depth = [], '', 0
    for ch in s:
        if ch == '(':
            depth += 1
        elif ch == ')':
            depth -= 1
        if ch.isspace() and depth == 0:
            if buf:
                out.append(buf)
                buf = ''
            continue
        buf += ch
    if buf:
        out.append(buf)
    return out


def forms(text):
    out, buf, depth = [], '', 0
    for ch in text:
        if depth == 0 and ch.isspace() and not buf:
            continue
        if depth == 0 and buf and ch.isspace():
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


def schemas(text):
    """op -> (input sorts, output sort)."""
    out = {}
    for f in forms(text):
        m = re.match(r'\((constructor|function)\s+([^\s()]+)\s*\(([^)]*)\)\s*([^\s()]+)', f)
        if m:
            out[m.group(2)] = (split_args(m.group(3).strip()), m.group(4))
            continue
        m = re.match(r'\(relation\s+([^\s()]+)\s*\(([^)]*)\)', f)
        if m:
            out[m.group(1)] = (split_args(m.group(2).strip()), None)
    return out


def top_sexps(text):
    """Split a rule body into its top-level s-expressions."""
    out, buf, depth = [], '', 0
    for ch in text:
        if depth == 0 and ch.isspace() and not buf:
            continue
        if depth == 0 and buf and ch.isspace():
            out.append(buf)
            buf = ''
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


def body_facts(body):
    """Yield (text, op, args) for every pattern fact in a rule body.

    A body item is either a bare fact `(Op a b)` or a binding `(= v (Op a b))`.
    Both are genuine table reads; the second form additionally names the result.
    """
    out = []
    for item in top_sexps(body):
        if not (item.startswith('(') and item.endswith(')')):
            continue
        inner = item[1:-1].strip()
        if inner.startswith('='):
            rest = inner[1:].strip()
            parts = top_sexps(rest)
            if len(parts) != 2 or not parts[1].startswith('('):
                continue
            item = parts[1]
            inner = item[1:-1].strip()
        m = re.match(r'([^()\s]+)\s*(.*)$', inner, re.S)
        if not m:
            continue
        op, argtext = m.group(1), m.group(2)
        out.append((item, op, split_args(argtext.strip())))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('source', type=pathlib.Path)
    ap.add_argument('out', type=pathlib.Path)
    ap.add_argument('--spec', action='append', default=[],
                    help='Op:col pairs; col is the position in the printed fact')
    a = ap.parse_args()

    text = a.source.read_text()
    fs = forms(text)
    sch = schemas(text)
    specs = []
    for s in a.spec:
        op, col = s.rsplit(':', 1)
        specs.append((op, int(col)))

    decls, builders = [], []
    build_args = {}
    for op, col in specs:
        if op not in sch:
            raise SystemExit(f'unknown relation {op}')
        ins, _ = sch[op]
        if col >= len(ins):
            raise SystemExit(f'{op}:{col} is an output column; only inputs can be keys')
        sort = ins[col]
        key = f'__k_{op}_{col}'
        decls.append(f'(relation {key} ({sort}))')
        # body: bind one op row, emit its key
        args = [f'__a{i}' for i in range(len(ins))]
        build_args[key] = (op, args, args[col])

    rewritten = 0
    first_use = {}
    build_rulesets = {}
    for op, col in specs:
        build_rulesets[f'__k_{op}_{col}'] = set()
    out = []
    for f in fs:
        if not f.startswith('(rule'):
            out.append(f)
            continue
        m = re.match(r'\(rule\s*\((.*)\)\s*\(\((.*?)\)\)\s*(:.*)?\)\s*$', f, re.S)
        if not m:
            out.append(f)
            continue
        body, head, tail = m.group(1), m.group(2), (m.group(3) or '')
        added = []
        ruleset = ''
        rm = re.search(r':ruleset\s+([^\s)]+)', tail)
        if rm:
            ruleset = rm.group(1)
        for _text, op, args in body_facts(body):
            for kop, col in specs:
                if op != kop or col >= len(args):
                    continue
                arg = args[col]
                if not re.fullmatch(r'[a-zA-Z_][\w\-]*', arg):
                    continue  # a literal is already const-pruned
                atom = f'(__k_{op}_{col} {arg})'
                if atom not in added:
                    added.append(atom)
                    first_use.setdefault(f'__k_{op}_{col}', len(out))
                    build_rulesets[f'__k_{op}_{col}'].add(ruleset)
        if added:
            rewritten += 1
            body = ' '.join(added) + ' ' + body
        out.append(f'(rule ({body}) (({head})) {tail})')

    # Declarations must precede use, and this generated program interleaves
    # declarations with rules, so each key relation is inserted immediately
    # before the first rule that reads it.
    merged = []
    pending = {name: text for name, text in zip(
        [f'__k_{op}_{col}' for op, col in specs], decls)}
    for i, f in enumerate(out):
        for name, text in list(pending.items()):
            if first_use.get(name) == i:
                merged.append(text)
                del pending[name]
        merged.append(f)
    assert not pending, f'key relation never used: {list(pending)}'
    # Builder rules must also be declared before the program's schedules, or
    # they never execute and the keys stay empty (which silently deletes every
    # derivation of the rewritten rules).
    for key, rulesets in build_rulesets.items():
        op, args, colarg = build_args[key]
        for rs in sorted(rulesets):
            rs_clause = f' :ruleset {rs}' if rs else ''
            builders.append(
                f'(rule ((= __r ({op} {" ".join(args)}))) (({key} {colarg}))'
                f'{rs_clause} :name "build_{key}_{rs or "default"}")'
            )
    sched = next((i for i, f in enumerate(merged) if f.startswith('(run-schedule')), len(merged))
    merged = merged[:sched] + builders + merged[sched:]
    a.out.write_text('\n'.join(merged) + '\n')
    print(f'specs={specs} rewritten_rules={rewritten} key_relations={len(decls)} -> {a.out}')


if __name__ == '__main__':
    main()

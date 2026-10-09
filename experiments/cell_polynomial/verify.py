#!/usr/bin/env python3
"""Exact factorized binding prototype, with a separate native egglog oracle."""
import argparse
import hashlib
import itertools
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
RULE = '(rewrite (Add a (Add b c)) (Add (Add a b) c))'


def zeros(m, n):
    return [[0] * n for _ in range(m)]


def mul(a, b):
    return [[sum(x * y for x, y in zip(row, col)) for col in zip(*b)] for row in a]


def add(a, b):
    return [[x + y for x, y in zip(ar, br)] for ar, br in zip(a, b)]


def outer(a, b):
    return [[x * y for y in b] for x in a]


class Model:
    def __init__(self, p, q):
        self.p, self.q = p, q
        self.n = len(q)
        self.classes = [{i} for i in range(self.n)]
        self.factors = []
        # J = sum over classes of (P u_C)(u_C^T Q).
        for c in self.classes:
            self.factors.append(self.project(c, c))

    def project(self, left, right):
        return ([sum(row[k] for k in left) for row in self.p],
                [sum(self.q[k][j] for k in right) for j in range(len(self.q[0]))])

    def union(self, a, b):
        ca = next(c for c in self.classes if a in c)
        cb = next(c for c in self.classes if b in c)
        if ca is cb:
            return 0
        self.factors.extend([self.project(ca, cb), self.project(cb, ca)])
        self.classes = [c for c in self.classes if c is not ca and c is not cb] + [ca | cb]
        return 2

    def matrix(self):
        # Expanded only by the verifier, not by union().
        result = zeros(len(self.p), len(self.q[0]))
        for l, r in self.factors:
            result = add(result, outer(l, r))
        return result

    def compact(self):
        # Disjoint class blocks absorb historical cross terms exactly.
        self.factors = [self.project(c, c) for c in self.classes]

    def reference(self):
        e = [[int(any(i in c and j in c for c in self.classes))
              for j in range(self.n)] for i in range(self.n)]
        return mul(mul(self.p, e), self.q)

    def bindings(self):
        # Independent explicit enumeration, deduplicated observable (a,b,c).
        return {(i, j) for i, row in enumerate(self.p) for a, present in enumerate(row)
                if present for b, qr in enumerate(self.q)
                if any(a in c and b in c for c in self.classes)
                for j, present_q in enumerate(qr) if present_q}

    def verify(self):
        matrix = self.matrix()
        assert matrix == self.reference()
        assert {(i, j) for i, row in enumerate(matrix) for j, v in enumerate(row) if v} == self.bindings()
        return matrix


def native_source(model):
    lines = ['(datatype Math (Var String) (Add Math Math))',
             '(relation Hit (Math Math Math))', RULE,
             '(rule ((Add a (Add b c))) ((Hit a b c)))']
    for k, row in enumerate(model.q):
        for j, present in enumerate(row):
            if present:
                lines.append(f'(union (Var "k{k}") (Add (Var "b{j}") (Var "c{j}")))')
    for i, row in enumerate(model.p):
        for k, present in enumerate(row):
            if present:
                lines.append(f'(Add (Var "a{i}") (Var "k{k}"))')
    for c in model.classes:
        first, *rest = sorted(c)
        for k in rest:
            lines.append(f'(union (Var "k{first}") (Var "k{k}"))')
    lines.append('(run 1)')
    hits = model.bindings()
    for i, j in itertools.product(range(len(model.p)), range(len(model.q[0]))):
        check = f'(check (Hit (Var "a{i}") (Var "b{j}") (Var "c{j}")))'
        lines.append(check if (i, j) in hits else f'(fail {check})')
        if (i, j) in hits:
            k = next(k for k, v in enumerate(model.p[i]) if v and any(
                k in c and any(model.q[b][j] for b in c) for c in model.classes))
            lines.append(f'(check (= (Add (Var "a{i}") (Var "k{k}")) '
                         f'(Add (Add (Var "a{i}") (Var "b{j}")) (Var "c{j}"))))')
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, default=ROOT/'out/cell-polynomial')
    parser.add_argument('--binary', type=Path, default=ROOT/'target/release/examples/native_math_baseline')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    source = ROOT/'egglog/tests/math-microbenchmark.egg'
    assert RULE in source.read_text()
    p = [[1,0,0,0], [0,1,0,0], [1,1,0,0], [0,0,1,0], [0,0,0,1]]
    q = [[1,0,0,0], [0,1,0,0], [0,0,1,0], [0,0,0,1]]
    model = Model(p, q)
    rows = []
    for phase, merge in enumerate([None, (0,1), (1,2), (0,0), (2,3)]):
        old = model.bindings()
        terms = model.union(*merge) if merge else 0
        matrix = model.verify()
        before = len(model.factors)
        model.compact()
        assert model.verify() == matrix
        egg = args.output/f'phase-{phase}.egg'
        egg.write_text(native_source(model))
        result = subprocess.run([str(args.binary.resolve()), str(egg.resolve())], capture_output=True, text=True)
        (args.output/f'phase-{phase}.log').write_text(result.stdout + result.stderr)
        assert result.returncode == 0, result.stderr
        rows.append(dict(phase=phase, union=merge, new_outer_terms=terms,
                         raw_witnesses=sum(map(sum, matrix)), distinct_bindings=len(model.bindings()),
                         newly_enabled=len(model.bindings()-old), max_coefficient=max(map(max, matrix)),
                         factors_before_compaction=before, factors_after_compaction=len(model.factors),
                         factor_scalar_slots=sum(len(l)+len(r) for l,r in model.factors),
                         dense_scalar_slots=len(p)*len(q[0]), matrix=matrix, native_checks_passed=True))
    # All ordered triples of union edges, including repeats and reversed edges.
    checks = 0
    for edges in itertools.product(list(itertools.combinations(range(4),2)), repeat=3):
        m = Model(p,q)
        for edge in edges:
            m.union(*edge); m.verify(); checks += 1
        m.compact(); m.verify()
    # Larger structured fixture: exact slot accounting, NOT memory benchmark.
    n = 64
    m = Model([[1,0] for _ in range(n)], [[0]*n, [1]*n])
    assert not m.bindings()
    m.union(0,1); m.verify(); m.compact()
    stats = dict(rows=n, cols=n, bindings=len(m.bindings()),
                 factor_scalar_slots=sum(len(l)+len(r) for l,r in m.factors), dense_scalar_slots=n*n)
    report = dict(scope='Standalone exact binding algebra, native egglog one-step oracle; no runtime acceleration claimed',
                  rule=RULE, source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                  binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                  union_sequence_checks=checks, phases=rows, structured_case=stats)
    (args.output/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()

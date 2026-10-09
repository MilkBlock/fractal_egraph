#!/usr/bin/env python3
"""Exact experiments: recursive associativity and full-rank scan operators."""
import argparse
from fractions import Fraction
from functools import lru_cache
import hashlib
import json
from math import comb
from pathlib import Path
import re
import subprocess
from verify import ROOT, RULE, mul


def rank(matrix):
    a = [[Fraction(v) for v in row] for row in matrix]
    r = 0
    for col in range(len(a[0])):
        pivot = next((i for i in range(r, len(a)) if a[i][col]), None)
        if pivot is None:
            continue
        a[r], a[pivot] = a[pivot], a[r]
        v = a[r][col]
        a[r] = [x / v for x in a[r]]
        for i in range(r + 1, len(a)):
            v = a[i][col]
            if v:
                a[i] = [x-v*y for x,y in zip(a[i], a[r])]
        r += 1
    return r


def interval_counts(n):
    # G[i,j] = sum_k z[i,k,j] * G[i,k] * G[k,j]. Evaluate z=1.
    counts = {(i,i+1): 1 for i in range(n)}
    for size in range(2,n+1):
        for i in range(n-size+1):
            j = i + size
            counts[i,j] = sum(counts[i,k]*counts[k,j] for k in range(i+1,j))
    return counts


def leaf(i):
    return f'(Var "x{i}")'


def right_comb(i,j):
    if j == i+1:
        return leaf(i)
    return f'(Add {leaf(i)} {right_comb(i+1,j)})'


@lru_cache(None)
def trees(i,j):
    if j == i+1:
        return (leaf(i),)
    return tuple(f'(Add {l} {r})' for k in range(i+1,j)
                 for l in trees(i,k) for r in trees(k,j))


def native_case(n, dest, binary):
    count = interval_counts(n)
    assert count[0,n] == comb(2*(n-1),n-1)//n
    seed = right_comb(0,n)
    lines = ['(datatype Math (Var String) (Add Math Math))', RULE, seed,
             '(run-schedule (saturate (run)))', '(print-size Add)']
    # One representative per split/interval; already present if interval closure is exact.
    checks = [(right_comb(i,j), f'(Add {right_comb(i,k)} {right_comb(k,j)})')
              for i in range(n) for j in range(i+2,n+1) for k in range(i+1,j)]
    if n <= 7:
        checks.extend((seed, tree) for tree in trees(0,n))
    lines.extend(f'(check (= {a} {b}))' for a,b in checks)
    lines.append('(print-size Add)')
    path = dest/f'assoc-{n}.egg'
    path.write_text('\n'.join(lines)+'\n')
    proc = subprocess.run([str(binary.resolve()), str(path.resolve()), 'shape'],
                          text=True, capture_output=True)
    (dest/f'assoc-{n}.log').write_text(proc.stdout+proc.stderr)
    assert proc.returncode == 0, proc.stderr
    sizes = list(map(int,re.findall(r'^\s*(\d+)\s*$',proc.stdout,re.M)))
    expected = comb(n+1,3)
    assert sizes == [expected,expected], sizes
    m = re.search(r'shape_enodes=(\d+) shape_eclasses=(\d+)',proc.stderr)
    assert m is not None
    nodes, classes = map(int,m.groups())
    assert nodes == expected+n
    assert classes == n*(n+1)//2
    return dict(leaves=n, equivalent_root_trees=count[0,n], add_enodes=expected,
                all_math_enodes=nodes, eclasses=classes, rhs_feedback='native saturation',
                all_root_trees_checked=n<=7, equality_checks=len(checks), native_checks_passed=True)


def guard_case(n):
    # Lower-triangular <= guard applied to a non-01 rank-one outer product.
    u, v = list(range(1,n+1)), list(range(2,n+2))
    raw = [[a*b for b in v] for a in u]
    masked = [[raw[i][j] if j<=i else 0 for j in range(n)] for i in range(n)]
    l = [[int(j<=i) for j in range(n)] for i in range(n)]
    d = [[int(i==j)-int(i==j+1) for j in range(n)] for i in range(n)]
    assert mul(l,d) == [[int(i==j) for j in range(n)] for i in range(n)]
    assert rank(raw) == 1
    assert rank(masked) == n
    xs = [[int(i==k) for i in range(n)] for k in range(n)]
    xs += [[(-1)**i*(i+3) for i in range(n)]]
    for x in xs:
        dense = [sum(a*b for a,b in zip(row,x)) for row in masked]
        acc = 0
        scan = []
        for a,b,c in zip(u,v,x):
            acc += b*c
            scan.append(a*acc)
        assert dense == scan
    return dict(dimension=n, raw_rank=1, guarded_rank=n,
                dense_entries=n*n, stored_numeric_coefficients=2*n,
                scan_multiplications=2*n, scan_additions=n-1,
                exact_basis_vector_checks=len(xs), inverse_nonzeros=2*n-1,
                scope='Exact integer algebra; no egglog guard runtime benchmark')


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--output',type=Path,default=ROOT/'out/cell-polynomial-explore')
    p.add_argument('--binary',type=Path,default=ROOT/'target/release/examples/native_math_baseline')
    args = p.parse_args()
    args.output.mkdir(parents=True,exist_ok=True)
    assert RULE in (ROOT/'egglog/tests/math-microbenchmark.egg').read_text()
    report = dict(scope='Standalone exact experiments, not a Cell runtime integration',
                  binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                  recursive_associativity=[native_case(n,args.output,args.binary) for n in [3,4,5,6,7,12]],
                  structured_full_rank=[guard_case(n) for n in [4,16,64]])
    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__ == '__main__':
    main()

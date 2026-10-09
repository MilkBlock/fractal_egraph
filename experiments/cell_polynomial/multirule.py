#!/usr/bin/env python3
"""AC + multiplication commutativity + distribution: exact finite Cell family."""
import argparse
from functools import lru_cache
import hashlib
import itertools
import json
from math import comb, factorial
from pathlib import Path
import re
import subprocess
from verify import ROOT

RULES = [
 '(rewrite (Add a b) (Add b a))',
 '(rewrite (Add a (Add b c)) (Add (Add a b) c))',
 '(rewrite (Mul a b) (Mul b a))',
 '(rewrite (Mul a (Add b c)) (Add (Mul a b) (Mul a c)))',
]


def splits(mask):
    part=(mask-1)&mask
    while part:
        yield part, mask^part
        part=(part-1)&mask


@lru_cache(None)
def tree_count(mask, scaled):
    # Root constructor is either a leaf/factored multiplication, or an Add split.
    if not scaled and mask.bit_count()==1:
        return 1
    base=2*tree_count(mask,False) if scaled else 0
    return base+sum(tree_count(a,scaled)*tree_count(b,scaled) for a,b in splits(mask))


def expression(mask):
    leaves=[f'(Var "a{i}")' for i in range(mask.bit_length()) if mask>>i&1]
    term=leaves[-1]
    for leaf in reversed(leaves[:-1]):
        term=f'(Add {leaf} {term})'
    return term


def shape(n):
    d=3**n-2**(n+1)+1
    return dict(leaves=n, add_enodes=2*d, mul_enodes=2*(2**n-1),
                math_enodes=2*d+2*(2**n-1)+n+1, eclasses=2**(n+1)-1,
                plain_root_trees=tree_count((1<<n)-1,False),
                scaled_root_trees=tree_count((1<<n)-1,True))


FACTOR = '(rewrite (Add (Mul a b) (Mul a c)) (Mul a (Add b c)))'


def run_case(n, mode, dest, binary, factoring=False):
    rules = RULES + ([FACTOR] if factoring else [])
    if mode=='combined':
        declarations=rules
        schedule='(run-schedule (saturate (run)))'
    else:
        declarations=['(ruleset ac)','(ruleset dist)']
        declarations += [r[:-1]+f' :ruleset {"dist" if i>=3 else "ac"})' for i,r in enumerate(rules)]
        schedule='(run-schedule (saturate (saturate (run ac)) (run dist)))'
    lines=['(datatype Math (Var String) (Add Math Math) (Mul Math Math))',*declarations]
    expected=[];stages=[]
    for phase,count in enumerate([n,n+1]):
        full=(1<<count)-1
        # Prepend a new Add child on phase 1; retain the previous saturated graph.
        term=expression(full) if not phase else f'(Add (Var "a{n}") {expression((1<<n)-1)})'
        lines.extend([f'(Mul (Var "x") {term})',schedule,'(print-size Add)','(print-size Mul)'])
        stats=shape(count);expected.extend([stats['add_enodes'],stats['mul_enodes']])
        plain={m:f'$p{phase}_u{m}' for m in range(1,full+1)}
        scaled={m:f'$p{phase}_s{m}' for m in range(1,full+1)}
        for m in plain:
            lines.append(f'(let {plain[m]} {expression(m)})')
            lines.append(f'(let {scaled[m]} (Mul (Var "x") {plain[m]}))')
        for m in plain:
            lines.append(f'(check (= {scaled[m]} (Mul {plain[m]} (Var "x"))))')
            for a,b in splits(m):
                lines.append(f'(check (= {plain[m]} (Add {plain[a]} {plain[b]})))')
                lines.append(f'(check (= {scaled[m]} (Add {scaled[a]} {scaled[b]})))')
        # No two distinct predicted polynomial signatures may canonicalize together.
        reps=[*plain.values(),*scaled.values(),'(Var "x")']
        for a,b in itertools.combinations(reps,2):
            lines.append(f'(fail (check (= {a} {b})))')
        lines.extend(['(print-size Add)','(print-size Mul)'])
        expected.extend([stats['add_enodes'],stats['mul_enodes']])
        stats['distinct_class_pairs_checked']=comb(len(reps),2)
        stages.append(stats)
    path=dest/f'{mode}-{n}-factor{int(factoring)}.egg';path.write_text('\n'.join(lines)+'\n')
    result=subprocess.run([str(binary.resolve()),str(path.resolve()),'shape'],capture_output=True,text=True)
    (dest/f'{mode}-{n}-factor{int(factoring)}.log').write_text(result.stdout+result.stderr)
    assert result.returncode==0,result.stderr
    counts=list(map(int,re.findall(r'^\s*(\d+)\s*$',result.stdout,re.M)))
    assert counts==expected,(counts,expected)
    s=re.search(r'shape_enodes=(\d+) shape_eclasses=(\d+)',result.stderr)
    assert s and tuple(map(int,s.groups()))==(stages[-1]['math_enodes'],stages[-1]['eclasses'])
    return dict(schedule=mode,factoring=factoring,stages=stages,native_checks_passed=True)


def main():
    p=argparse.ArgumentParser()
    p.add_argument('--output',type=Path,default=ROOT/'out/cell-polynomial-multirule')
    p.add_argument('--binary',type=Path,default=ROOT/'target/release/examples/native_math_baseline')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    source=(ROOT/'egglog/tests/math-microbenchmark.egg').read_text()
    assert all(r in source for r in RULES+[FACTOR])
    # Independently verify the coupled exponential generating function coefficients:
    # F=z+F^2; G=2F+G^2. Stored coefficients are f_n/n!, g_n/n!.
    f=[0,1];g=[0,2]
    for n in range(2,9):
        f.append(sum(f[k]*f[n-k] for k in range(1,n)))
        g.append(2*f[n]+sum(g[k]*g[n-k] for k in range(1,n)))
    for n in range(1,9):
        assert f[n]*factorial(n)==tree_count((1<<n)-1,False)
        assert g[n]*factorial(n)==tree_count((1<<n)-1,True)
    runs=[run_case(n,mode,args.output,args.binary,factoring) for n in [3,4]
          for factoring in [False,True] for mode in ['combined','alternating']]
    for a,b in zip(runs[::2],runs[1::2]):
        assert a['stages']==b['stages']
    for n in [3,4]:
        group=[run['stages'] for run in runs if run['stages'][0]['leaves']==n]
        assert all(stages==group[0] for stages in group)
    report=dict(scope='Four/five-rule specialized closure descriptor; actual native saturation oracle, no accelerated matcher',
                rules=RULES, optional_factoring_rule=FACTOR, source_sha256=hashlib.sha256(source.encode()).hexdigest(),
                binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),runs=runs,
                egf_coefficients=dict(F=f[1:],G=g[1:]),
                native_root_tree_counts='Computed from verified acyclic split grammar; not individually extracted')
    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__=='__main__':
    main()

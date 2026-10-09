#!/usr/bin/env python3
"""Finite-depth integral family with shared binding expressions and native oracle."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
from verify import ROOT

IBP='(rewrite (Integral (Mul a b) x) (Sub (Mul a (Integral b x)) (Integral (Mul (Diff x a) (Integral b x)) x)))'
LINEAR='(rewrite (Integral (Add f g) x) (Add (Integral f x) (Integral g x)))'
COMM='(rewrite (Mul a b) (Mul b a))'

# Expressions: (base variable, unary constructor word, outermost last).
def D(a): return (a[0],a[1]+'D')
def I(a): return (a[0],a[1]+'I')
def canonical(a,b,comm): return tuple(sorted((a,b))) if comm else (a,b)
def orientations(pair,comm): return {pair,pair[::-1]} if comm else {pair}

def family(depth,comm):
    level={canonical(('f',''),('g',''),comm),canonical(('h',''),('g',''),comm)}
    layers=[level]
    for _ in range(depth):
        level={canonical(D(a),I(b),comm) for pair in level for a,b in orientations(pair,comm)}
        layers.append(level)
    return layers


def expr(term):
    result=f'(Var "{term[0]}")'
    for op in term[1]:
        result=f'(Diff (Var "x") {result})' if op=='D' else f'(Integral {result} (Var "x"))'
    return result


def run_case(depth,comm,out,binary):
    layers=family(depth,comm)
    states=set().union(*layers)
    ordered={orientation for pair in states for orientation in orientations(pair,comm)}
    expanded=set().union(*layers[:-1]) if depth else set()
    declarations=['(datatype Math (Var String) (Add Math Math) (Mul Math Math) (Sub Math Math) (Diff Math Math) (Integral Math Math))',
                  '(ruleset linear)','(ruleset ibp)','(ruleset orient)','(ruleset observe)',
                  LINEAR[:-1]+' :ruleset linear)',IBP[:-1]+' :ruleset ibp)',
                  '(relation State (Math Math))',
                  '(rule ((Integral (Mul a b) x)) ((State a b)) :ruleset observe)']
    if comm:declarations.append(COMM[:-1]+' :ruleset orient)')
    lines=declarations+['(Integral (Add (Mul (Var "f") (Var "g")) (Mul (Var "h") (Var "g"))) (Var "x"))',
                       '(run-schedule (saturate (run linear)))']
    for _ in range(depth):
        lines.extend(['(run-schedule (saturate (run orient)))','(run ibp 1)'])
    lines.extend(['(run-schedule (saturate (run orient)))','(run observe 1)'])
    functions=['State','Add','Mul','Sub','Diff','Integral','Var']
    lines.extend(f'(print-size {name})' for name in functions)
    terms={term for pair in ordered for term in pair}
    names={term:f'$t{i}' for i,term in enumerate(sorted(terms))}
    lines.extend(f'(let {names[t]} {expr(t)})' for t in sorted(terms))
    for a,b in sorted(ordered):
        lines.append(f'(check (State {names[a]} {names[b]}))')
    # Verify actual RHS effects of every expanded binding, not just its label.
    for pair in sorted(expanded):
        for a,b in sorted(orientations(pair,comm)):
            aa,bb=names[a],names[b]
            lines.append(f'(check (= (Integral (Mul {aa} {bb}) (Var "x")) '
                         f'(Sub (Mul {aa} (Integral {bb} (Var "x"))) '
                         f'(Integral (Mul (Diff (Var "x") {aa}) (Integral {bb} (Var "x"))) (Var "x")))))')
    # No accidental equality between the unary binding terms.
    terms_sorted=sorted(terms)
    for idx,a in enumerate(terms_sorted):
        for b in terms_sorted[idx+1:]:
            lines.append(f'(fail (check (= {names[a]} {names[b]})))')
    lines.extend(f'(print-size {name})' for name in functions)
    path=out/f'integral-d{depth}-comm{int(comm)}.egg';path.write_text('\n'.join(lines)+'\n')
    proc=subprocess.run([str(binary.resolve()),str(path.resolve()),'shape'],capture_output=True,text=True)
    (path.with_suffix('.log')).write_text(proc.stdout+proc.stderr)
    assert proc.returncode==0,proc.stderr
    counts=list(map(int,re.findall(r'^\s*(\d+)\s*$',proc.stdout,re.M)))
    assert len(counts)==2*len(functions),counts
    before,after=counts[:len(functions)],counts[len(functions):]
    assert before==after,(before,after)
    assert before[0]==len(ordered),(before[0],len(ordered))
    return dict(depth=depth,mul_commutativity=comm,states_per_depth=list(map(len,layers)),
                distinct_normalized_states=len(states),ordered_binding_states=len(ordered),
                distinct_unary_terms=len(terms),table_counts=dict(zip(functions,before)),
                native_checks_passed=True)


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--output',type=Path,default=ROOT/'out/cell-polynomial-integral')
    parser.add_argument('--binary',type=Path,default=ROOT/'target/release/examples/native_math_baseline')
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    source=(ROOT/'egglog/tests/math-microbenchmark.egg').read_text()
    normalized=' '.join(source.split())
    assert all(rule in normalized for rule in [IBP,LINEAR,COMM])
    runs=[run_case(depth,comm,args.output,args.binary) for comm in [False,True] for depth in range(5)]
    report=dict(scope='Exact bounded integral rewrite family, no saturation or runtime speedup claim',
                rules=[LINEAR,IBP,COMM],source_sha256=hashlib.sha256(source.encode()).hexdigest(),
                binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),runs=runs)
    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__=='__main__':main()

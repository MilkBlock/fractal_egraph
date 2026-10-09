#!/usr/bin/env python3
"""Sequential native grafts vs an implicit ordered-interval Cell."""
import argparse
from math import comb
from pathlib import Path
import json
import re
import subprocess
from verify import ROOT, RULE, mul
from explore import right_comb


def choose(n,k):
    return comb(n,k) if n>=k else 0


def upper(n):
    return [[int(i<j) for j in range(n)] for i in range(n)]


def prepend_count_response(n):
    # Existing n leaves have n+1 boundaries. Prepend one new boundary.
    # U_new = [[0,r], [0,U]] with r = all ones (new intervals).
    u = upper(n+1)
    r = [[1]*(n+1)]
    add_response = mul(r,u)[0]
    hit_response = mul(mul(r,u),u)[0]
    assert add_response == list(range(n+1))
    assert hit_response == [choose(j,2) for j in range(n+1)]
    assert sum(add_response) == choose(n+1,2)
    assert sum(hit_response) == choose(n+1,3)
    # Verify the full old block of both powers is unchanged.
    new = upper(n+2)
    for old_power,new_power in [(mul(u,u),mul(new,new)),
                                 (mul(mul(u,u),u),mul(mul(new,new),new))]:
        assert [row[1:] for row in new_power[1:]] == old_power
    return sum(add_response),sum(hit_response)


def case(initial, steps, dest, binary):
    stop = initial+steps
    lines = ['(datatype Math (Var String) (Add Math Math))', RULE,
             '(relation Hit (Math Math Math Math))',
             '(rule ((= root (Add a (Add b c)))) ((Hit root a b c)))']
    rows=[]
    expected=[]
    for phase,start in enumerate(range(steps,-1,-1)):
        n = stop-start
        # Same engine: old saturated suffix stays; just a new parent is added.
        lines.append(right_comb(start,stop))
        lines.extend(['(run-schedule (saturate (run)))', '(print-size Add)', '(print-size Hit)'])
        adds,hits=choose(n+1,3),choose(n+1,4)
        expected.extend([adds,hits])
        if phase:
            da,dh=prepend_count_response(n-1)
            assert da==adds-rows[-1]['add_enodes']
            assert dh==hits-rows[-1]['distinct_hits']
        else:
            da=dh=None
        # Every expected interval split and every expected four-boundary match.
        # Counts before and after ensure checks have not materialized missing nodes.
        refs = {(i,j): f'$p{phase}_{i}_{j}' for i in range(start,stop)
                for j in range(i+1,stop+1)}
        lines.extend(f'(let {name} {right_comb(i,j)})' for (i,j),name in refs.items())
        for i in range(start,stop):
            for k in range(i+1,stop):
                for j in range(k+1,stop+1):
                    lines.append(f'(check (= {refs[i,j]} (Add {refs[i,k]} {refs[k,j]})))')
                    for l in range(k+1,j):
                        lines.append(f'(check (Hit {refs[i,j]} {refs[i,k]} '
                                     f'{refs[k,l]} {refs[l,j]}))')
        lines.extend(['(print-size Add)', '(print-size Hit)'])
        expected.extend([adds,hits])
        rows.append(dict(phase=phase,leaves=n,add_enodes=adds,distinct_hits=hits,
                         new_add_enodes=da,new_distinct_hits=dh,
                         implicit_state='ordered distinct leaf list + interval/split schema'))
    path=dest/f'prepend-{initial}.egg'
    path.write_text('\n'.join(lines)+'\n')
    proc=subprocess.run([str(binary.resolve()),str(path.resolve()),'shape'],capture_output=True,text=True)
    (dest/f'prepend-{initial}.log').write_text(proc.stdout+proc.stderr)
    assert proc.returncode==0,proc.stderr
    sizes=list(map(int,re.findall(r'^\s*(\d+)\s*$',proc.stdout,re.M)))
    assert sizes==expected,(sizes,expected)
    n=initial+steps
    m=re.search(r'shape_enodes=(\d+) shape_eclasses=(\d+)',proc.stderr)
    assert m and tuple(map(int,m.groups()))==(choose(n+1,3)+n,choose(n+1,2))
    return dict(initial_leaves=initial,successive_grafts=steps,phases=rows,native_checks_passed=True)


def main():
    p=argparse.ArgumentParser()
    p.add_argument('--output',type=Path,default=ROOT/'out/cell-polynomial-feedback')
    p.add_argument('--binary',type=Path,default=ROOT/'target/release/examples/native_math_baseline')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    assert RULE in (ROOT/'egglog/tests/math-microbenchmark.egg').read_text()
    report=dict(scope='Specialized associativity interval model with sequential native saturation oracle',
                cases=[case(n,3,args.output,args.binary) for n in [3,7,12]],
                algebra_checks=[dict(old_leaves=n,delta_add=prepend_count_response(n)[0],
                                     delta_hits=prepend_count_response(n)[1]) for n in range(2,25)])
    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__=='__main__':
    main()

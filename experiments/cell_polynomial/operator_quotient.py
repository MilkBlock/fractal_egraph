#!/usr/bin/env python3
"""Exact IBP operator quotient on anchored polynomial integration.

This is NOT the existing egglog opaque D/I syntax semantics.
"""
import argparse
from fractions import Fraction
from itertools import product
import json
from math import factorial
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
IDENTITY=(0,0,0)  # delta, negative prefix minimum, positive prefix maximum


def compose(first, second):
    """Execute first, then second."""
    d,a,b=first
    e,c,f=second
    return d+e,max(a,c-d),max(b,f+d)


def power(s,n):
    if n==0:return IDENTITY
    d,a,b=s
    return n*d,a+(n-1)*max(-d,0),b+(n-1)*max(d,0)


def summary(word):
    d=a=b=0
    for step in word:
        d+=step
        a=max(a,-d)
        b=max(b,d)
    return d,a,b


def execute(word,k,l):
    """Literal monomial action; integration is from 0, derivative of 1 is 0."""
    coefficient=Fraction(1)
    for step in word:
        if step==-1:
            if k==0:return None
            coefficient*=Fraction(k,l+1)
            k-=1;l+=1
        else:
            if l==0:return None
            coefficient*=Fraction(l,k+1)
            k+=1;l-=1
    return coefficient,k,l


def apply_summary(s,k,l):
    d,a,b=s
    if k<a or l<b:return None
    return Fraction(factorial(k)*factorial(l),factorial(k+d)*factorial(l-d)),k+d,l-d


def effect_sum(word,k,l):
    """Coefficient of x^(k+l+1) in all signed IBP boundary products."""
    coefficient=Fraction(1)
    acc=Fraction(0)
    for n,step in enumerate(word):
        divisor=l+1 if step==-1 else k+1
        acc+=(-1)**n*coefficient/divisor
        result=execute([step],k,l)
        if result is None:
            return acc,None
        factor,k,l=result
        coefficient*=factor
    return acc,(coefficient,k,l)


def main():
    p=argparse.ArgumentParser()
    p.add_argument('--output',type=Path,default=ROOT/'out/cell-operator-quotient')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    action_checks=effect_checks=0
    # All words up through length 9 on 64 monomial-pair inputs.
    for n in range(10):
        for word in product([-1,1],repeat=n):
            s=summary(word)
            for k,l in product(range(8),repeat=2):
                expected=execute(word,k,l)
                assert apply_summary(s,k,l)==expected
                action_checks+=1
                acc,residual=effect_sum(word,k,l)
                expected_acc=Fraction(1,k+l+1)
                if residual is not None:
                    expected_acc-=(-1)**n*residual[0]/(k+l+1)
                assert acc==expected_acc
                effect_checks+=1
    words=[word for n in range(6) for word in product([-1,1],repeat=n)]
    composition_checks=0
    for first in words:
        for second in words:
            assert compose(summary(first),summary(second))==summary(first+second)
            composition_checks+=1
    # Algebra-law checks on reachable summaries, including boundary effects.
    summaries={summary(word) for word in words}
    for s in summaries:
        assert compose(s,IDENTITY)==compose(IDENTITY,s)==s
    associativity_checks=0
    for a,b,c in product(summaries,repeat=3):
        assert compose(compose(a,b),c)==compose(a,compose(b,c))
        associativity_checks+=1
    assert summary([-1,1])!=(summary([1,-1]))
    assert apply_summary(summary([-1,1]),0,2) is None
    assert apply_summary(summary([1,-1]),0,2)==(Fraction(1),0,2)
    power_checks=0
    for s in summaries:
        accumulated=IDENTITY
        for n in range(21):
            assert power(s,n)==accumulated
            accumulated=compose(accumulated,s)
            power_checks+=1
        if s[0]==0:
            assert compose(s,s)==s
    levels={IDENTITY}; growth=[]
    for n in range(41):
        if n in [0,1,2,4,8,12,16,20,32,40]:
            growth.append(dict(depth=n,history_words=2**n,distinct_operators=len(levels)))
        levels={compose(s,(step,int(step<0),int(step>0))) for s in levels for step in [-1,1]}
    # Fixed degree M: exact finite matrices with rational, non-01 entries.
    m=4;minus=[[Fraction(0) for _ in range(m+1)] for _ in range(m+1)]
    plus=[[Fraction(0) for _ in range(m+1)] for _ in range(m+1)]
    for k in range(m+1):
        for step,matrix in [(-1,minus),(1,plus)]:
            result=execute([step],k,m-k)
            if result is not None:matrix[result[1]][k]=result[0]
    def matvec(a,v):return [sum(x*y for x,y in zip(row,v)) for row in a]
    for k in range(m+1):
        v=[Fraction(int(j==k)) for j in range(m+1)]
        left=matvec(minus,matvec(plus,v));right=matvec(plus,matvec(minus,v))
        assert [a-b for a,b in zip(left,right)]==[v[0],0,0,0,-v[-1]]
    report=dict(scope='Exact operator semantics with anchored integral; not native opaque egglog graph equivalence',
                action_checks=action_checks,effect_checks=effect_checks,
                composition_checks=composition_checks,associativity_checks=associativity_checks,
                power_checks=power_checks,
                growth=growth,degree=m,minus_matrix=[[str(v) for v in row] for row in minus],
                plus_matrix=[[str(v) for v in row] for row in plus])
    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__=='__main__':main()

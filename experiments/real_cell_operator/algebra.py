"""Exact row-space arithmetic for observed-Cell affine interfaces."""
from fractions import Fraction


def coordinates(basis,row):
    """Solve c*basis=row over Q. Return None if row is outside the span."""
    if not basis:return [] if not any(row) else None
    unknowns=len(basis)
    a=[[Fraction(basis[j][i]) for j in range(unknowns)]+[Fraction(row[i])] for i in range(len(row))]
    pivot_row=0;pivots=[]
    for col in range(unknowns):
        pivot=next((i for i in range(pivot_row,len(a)) if a[i][col]),None)
        if pivot is None:continue
        a[pivot_row],a[pivot]=a[pivot],a[pivot_row]
        value=a[pivot_row][col];a[pivot_row]=[x/value for x in a[pivot_row]]
        for i in range(len(a)):
            if i!=pivot_row:
                value=a[i][col]
                a[i]=[x-value*y for x,y in zip(a[i],a[pivot_row])]
        pivots.append(col);pivot_row+=1
    if any(not any(r[:-1]) and r[-1] for r in a):return None
    result=[Fraction(0)]*unknowns
    for i,col in enumerate(pivots):result[col]=a[i][-1]
    return result


def interface_basis(rows):
    """Known homogeneous constant first, then independent actual Cell outputs."""
    assert rows and all(len(r)==len(rows[0]) for r in rows)
    constant=[0]*(len(rows[0])-1)+[1]
    basis=[constant];owners=['constant']
    for i,row in enumerate(rows):
        if coordinates(basis,row) is None:basis.append(row);owners.append(i)
    transforms=[coordinates(basis,row) for row in rows]
    for row,cs in zip(rows,transforms):
        assert [sum(c*b[j] for c,b in zip(cs,basis)) for j in range(len(row))]==row
    return dict(basis=basis,owners=owners,rank=len(basis),
                transforms=[[str(c) for c in cs] for cs in transforms])


def shared_affine_program(rows,input_name='cell_size'):
    """Emit exact i64 arithmetic using the row basis, not duplicated formulas.

    This prototype accepts nonnegative integer reconstructions only. Rational
    coordinates remain useful algebraically but need a divisibility contract
    before they can become egglog integer division.
    """
    if any(len(row)!=2 for row in rows):raise ValueError('one scalar boundary required')
    basis=interface_basis(rows)

    def expression(coefficients,names):
        terms=[]
        for coefficient,name in zip(coefficients,names):
            coefficient=Fraction(coefficient)
            if coefficient.denominator!=1 or not 0<=coefficient<=(1<<63)-1:
                raise ValueError('integer nonnegative reconstruction required for native i64 code')
            value=int(coefficient)
            if value:
                terms.append(str(value) if name=='1' else name if value==1 else f'(* {value} {name})')
        result=terms[0] if terms else '0'
        for term in terms[1:]:result=f'(+ {result} {term})'
        return result

    names=['1']+[f'cell_basis_{i}' for i in range(1,len(basis['basis']))]
    lets=[f'(let {name} {expression(row,[input_name,"1"])})'
          for name,row in zip(names[1:],basis['basis'][1:])]
    outputs=[expression(coordinates,names) for coordinates in basis['transforms']]
    return lets,outputs,basis

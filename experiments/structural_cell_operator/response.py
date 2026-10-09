"""Exact matrix factorization of structural boundary-response polynomials."""
from fractions import Fraction
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'real_cell_operator'))
from algebra import interface_basis


class Response:
    def __init__(self, observations):
        self.names = list(observations)
        self.monomials = sorted({m for p in observations.values() for m in p.terms if m}) + [0]
        self.rows = [[p.terms.get(m,0) for m in self.monomials] for p in observations.values()]
        factor = interface_basis(self.rows)
        self.basis = factor['basis']
        self.reconstruction = [[Fraction(c) for c in row] for row in factor['transforms']]
        self.owners = ['constant' if i=='constant' else self.names[i] for i in factor['owners']]

    def evaluate(self, assignment):
        features = [int(assignment & m == m) for m in self.monomials]
        basis_values = [sum(c*x for c,x in zip(row,features)) for row in self.basis]
        values = [sum(c*x for c,x in zip(row,basis_values)) for row in self.reconstruction]
        if any(value not in (0,1) for value in values): raise ValueError('structural response is not Boolean')
        return dict(zip(self.names,map(int,values)))

    def report(self):
        return dict(observables=len(self.names),monomials=self.monomials,rank=len(self.basis),
                    basis=self.basis,basis_observables=self.owners,
                    signed_coefficients=sorted({c for row in self.rows for c in row if c not in (0,1)}),
                    note='Exact factorization of fact/equality response, not numerical e-class IDs or count fitting.')

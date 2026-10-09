"""Core semantic unit tests; real Cell evidence is the separate native runner."""
import unittest

from closure import Closure
from polynomial import Poly, ONE
from response import Response


def call(name,*args):return dict(kind='call',head=name,args=list(args))
def literal(n):return dict(kind='literal',sort='i64',text=str(n),value=n)
def var(name):return dict(kind='var',name=name)
def leaf(n):return call('Leaf',literal(n))
def union(a,b):return dict(kind='union',args=[a,b])
SIGS={
    name:dict(kind='constructor',name=name,input=inputs,output='Math')
    for name,inputs in [('Leaf',['i64']),('F',['Math']),('Pair',['Math','Math'])]
}


class ClosureTests(unittest.TestCase):
    def test_union_operand_materializes_the_constructor(self):
        c=Closure(SIGS,[]);u=Poly.var(0)
        a=c.construct(leaf(0),{},ONE)
        c.action(union(leaf(0),call('F',leaf(1))),u);c.saturate()
        f=next(n for n,t in enumerate(c.terms) if t.op=='F')
        self.assertEqual(c.present[f],u)
        self.assertEqual(c.equality(a,f),u)

    def test_congruence_and_transitive_union_are_conditional(self):
        c=Closure(SIGS,[]);p,q=Poly.var(0),Poly.var(1)
        a,b,d=[c.construct(leaf(n),{},ONE) for n in range(3)]
        fa,fd=[c.construct(call('F',leaf(n)),{},ONE) for n in (0,2)]
        c.union(a,b,p);c.union(b,d,q);c.saturate()
        self.assertEqual(c.equality(a,d),p*q)
        self.assertEqual(c.equality(fa,fd),p*q)

    def test_repeated_pattern_variable_is_an_equality_guard(self):
        # Pair(x,x) -> F(x) is a schema test, not a fabricated real Cell result.
        rule=dict(name='same-pair',body=[dict(kind='eq',args=[var('r'),call('Pair',var('x'),var('x'))])],
                  head=[union(var('r'),call('F',var('x')))])
        c=Closure(SIGS,[rule]);u=Poly.var(0)
        a,b=[c.construct(leaf(n),{},ONE) for n in range(2)]
        c.construct(call('Pair',leaf(0),leaf(1)),{},ONE)
        c.union(a,b,u);c.saturate()
        fs=[n for n,t in enumerate(c.terms) if t.op=='F']
        self.assertTrue(fs)
        self.assertTrue(all(c.present[n]==u for n in fs))

    def test_equal_children_make_an_existing_row_available_under_an_alias(self):
        c=Closure(SIGS,[]);p,q=Poly.var(0),Poly.var(1)
        a,b=[c.construct(leaf(n),{},ONE) for n in range(2)]
        fa=c.construct(call('F',leaf(0)),{},p)
        fb=c.construct(call('F',leaf(1)),{},ONE)
        c.union(a,b,q);c.saturate()
        self.assertEqual(c.present[fa],p|q)
        self.assertEqual(c.equality(fa,fb),q)

    def test_compact_effect_is_quotient_rows_not_term_spellings(self):
        c=Closure(SIGS,[])
        a,b=[c.construct(leaf(n),{},ONE) for n in range(2)]
        c.construct(call('F',leaf(0)),{},ONE);c.construct(call('F',leaf(1)),{},ONE)
        c.union(a,b,ONE);c.saturate()
        packet=c.compiled_actions(0)
        self.assertEqual(len(packet),3)  # two Leaf rows and one canonical F row
        self.assertEqual(sum('(F ' in a for a in packet),1)

    def test_cyclic_quotient_needs_a_different_emitter(self):
        c=Closure(SIGS,[])
        a=c.construct(leaf(0),{},ONE);f=c.construct(call('F',leaf(0)),{},ONE)
        c.union(a,f,ONE);c.saturate()
        with self.assertRaisesRegex(ValueError,'cyclic class graph'):
            c.compiled_actions(0)

    def test_nonlinear_response_factors_exactly_and_delta_is_not_just_or(self):
        p,q,r=map(Poly.var,range(3))
        observations={'union':r,'row':p|q,'cross':r*(p|q),'always':ONE}
        matrix=Response(observations)
        for world in range(8):
            self.assertEqual(matrix.evaluate(world),{n:f.evaluate(world) for n,f in observations.items()})
        self.assertIn(-1,matrix.report()['signed_coefficients'])
        self.assertEqual(observations['cross'].difference(2),p|q)

    def test_function_updates_reject_instead_of_becoming_constructor_effects(self):
        rule=dict(name='set',body=[dict(kind='eq',args=[var('r'),call('F',var('x'))])],
                  head=[dict(kind='set',head='size',args=[var('r')],value=literal(1))])
        with self.assertRaisesRegex(ValueError,'union effects'):Closure(SIGS,[rule])

    def test_erasing_negative_coefficient_breaks_structural_semantics(self):
        p,q=Poly.var(0),Poly.var(1)
        wrong=Response({'row':p+q})
        with self.assertRaisesRegex(ValueError,'not Boolean'):wrong.evaluate(3)


if __name__=='__main__':unittest.main()

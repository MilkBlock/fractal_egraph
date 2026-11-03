import unittest
import tempfile
from pathlib import Path
from features import Graph,key_signature,constructors


def v(n):return ('Eq','Expr',str(n))
OPS={'Leaf':([], 'Expr'),'Add':(['Expr','Expr'],'Expr'),'Mul':(['Expr','Expr'],'Expr')}


class FeatureTests(unittest.TestCase):
    def test_alpha_renaming_does_not_change_coordinates(self):
        a=Graph([('Leaf',(),v(1)),('Add',(v(1),v(2)),v(3))],OPS).tokens(2)
        b=Graph([('Leaf',(),v(71)),('Add',(v(71),v(82)),v(93))],OPS).tokens(2)
        self.assertEqual(key_signature((v(1),v(3)),a),key_signature((v(71),v(93)),b))

    def test_binding_order_alias_and_graph_alias_survive(self):
        a=Graph([('Leaf',(),v(1)),('Add',(v(1),v(2)),v(3))],OPS).tokens(2)
        b=Graph([('Leaf',(),v(1)),('Add',(v(1),v(1)),v(3))],OPS).tokens(2)
        self.assertNotEqual(key_signature((v(1),v(3)),a),key_signature((v(3),v(1)),a))
        self.assertNotEqual(key_signature((v(3),),a),key_signature((v(3),),b))
        self.assertNotEqual(key_signature((v(8),v(9)),a),key_signature((v(8),v(8)),a))

    def test_analysis_result_cannot_be_an_input_feature(self):
        raw=[('Leaf',(),v(1)),('Add',(v(1),v(2)),v(3))]
        a=Graph(raw,OPS).tokens(2)
        b=Graph(raw+[('ExprIsPure',(v(1),),('Eq','Unit','0'))],OPS).tokens(2)
        self.assertEqual(a,b)

    def test_native_relation_lowering_is_not_a_structural_constructor(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp)/'flat.egg'
            p.write_text('(constructor Add (Expr Expr) Expr)\n'
                '(constructor ExprIsPure (Expr) ___ExprIsPureSort)\n'
                '(constructor ___GeneratedFreshTable0 (Expr) Assumption)\n')
            self.assertEqual(constructors(p),{'Add':(['Expr','Expr'],'Expr')})

    def test_all_constructor_alternatives_and_nonroot_ports_retained(self):
        a=Graph([('Add',(v(1),v(2)),v(3)),('Mul',(v(1),v(2)),v(3)),('Leaf',(),v(1))],OPS).tokens(2)
        self.assertIn(('root','Add'),a[v(3)]);self.assertIn(('root','Mul'),a[v(3)])
        self.assertIn(('port','Add',1),a[v(2)])

    def test_undeclared_pattern_is_not_invented(self):
        g=Graph([('Leaf',(),v(1)),('Add',(v(1),v(2)),v(3))],OPS)
        only={('root','Add')};t=g.tokens(2,only)
        self.assertEqual(t[v(3)],only);self.assertFalse(t.get(v(1)))

    def test_parameters_at_observed_ports_survive_opaque_slot_erasure(self):
        ops={'Tag':([], 'Op'),'Bop':(['Op','Expr','i64'],'Expr')}
        tag=('Eq','Op','91');zero=('Base','i64','0')
        g=Graph([('Tag',(),tag),('Bop',(tag,v(1),zero),v(2))],ops)
        t=g.tokens(2,{('root','Bop')},parameters=True)
        self.assertIn(('enum-parameter','Bop',0,'Tag'),t[v(2)])
        self.assertIn(('literal-parameter','Bop',2,'integer-small',0),t[v(2)])


if __name__=='__main__':unittest.main()

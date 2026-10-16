"""Shared matching computation, with native full-binding and negative checks."""
from pathlib import Path
import json
import tempfile
import unittest
from unittest.mock import patch

from .demo import ROOT, RULES, Patterns, Matcher, run, class_index, native_matches
from experiments.cell_spine.factorized_boundary.demo import Family, Graph, native, sexpr


class SharedMatching(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix='shared-matching-')
        cls.addClassCleanup(cls.temp.cleanup)
        cls.out = Path(cls.temp.name)/'run'
        # Covers the whole pipeline: source extraction, matrices and native checks.
        with (patch.object(Family,'enumerate',side_effect=AssertionError('expanded Spine')),
              patch.object(Graph,'enumerate_terms',side_effect=AssertionError('expanded graph'))):
            cls.report = run(cls.out,skip_build=True)

    def test_native_all_rules_all_positions_and_boundary(self):
        r = self.report
        self.assertEqual(len(r['patterns']),4)
        self.assertTrue(r['native_full_binding_equal'])
        self.assertTrue(r['boundary']['native_equal'])
        self.assertEqual(r['candidate_spines_enumerated'],0)
        self.assertEqual(r['unified_shape'],[72,26])
        self.assertEqual(r['template_qualified_bindings'],40)

    def test_root_result_reused_as_non_root_in_other_cell(self):
        r = self.report
        factoring = next(p['id'] for p in r['patterns'] if p['text']=='(Add (Mul v0 v1) (Mul v0 v2))')
        occurrences = [x for x in r['results'] if x['spine']==factoring and x['bindings']]
        self.assertEqual(len(occurrences),3)
        small = next(x for x in occurrences if x['cell']==0)
        inner = next(x for x in occurrences if x['cell']==1 and x['location']=='non-root')
        self.assertEqual(small['anchor'],inner['anchor'])
        self.assertEqual(small['bindings'],inner['bindings'])
        self.assertEqual(len(small['bindings']),2)
        self.assertTrue(inner['path'])

    def test_actual_computation_counts_reduce_with_sharing(self):
        r = self.report
        a,b,c = [r['ablations'][s] for s in ('none','per_cell','global')]
        self.assertEqual([x['constructor_rows_read'] for x in (a,b,c)],[86,28,18])
        self.assertEqual([x['join_matrix_calls'] for x in (a,b,c)],[9,9,6])
        self.assertEqual([x['matrix_multiply_adds'] for x in (a,b,c)],[360,360,240])
        self.assertEqual(r['boundary']['cached_stats']['constructor_rows_read'],1)
        self.assertEqual(r['boundary']['uncached_stats']['constructor_rows_read'],6)

    def test_alpha_rename_preserves_wiring_and_different_alias_does_not_merge(self):
        p = Patterns()
        left = ('Add',('Mul',('V','a'),('V','b')),('Mul',('V','a'),('V','c')))
        renamed = ('Add',('Mul',('V','x'),('V','y')),('Mul',('V','x'),('V','z')))
        independent = ('Add',('Mul',('V','x'),('V','y')),('Mul',('V','z'),('V','w')))
        i = p.add(left)
        self.assertEqual(i,p.add(renamed))
        self.assertNotEqual(i,p.add(independent))
        self.assertEqual(p.nodes[i]['children'][0]['ports'],[0,1])
        self.assertEqual(p.nodes[i]['children'][1]['ports'],[0,2])

    def test_present_shapes_with_wrong_shared_binding_do_not_match(self):
        p = Patterns()
        pid = p.add(('Add',('Mul',('V','k'),('V','u')),('Mul',('V','k'),('V','v'))))
        index = {('Add',9):((9,4,5),),('Mul',4):((4,0,1),),('Mul',5):((5,2,3),)}
        engine = Matcher(p,index)
        self.assertEqual(engine.evaluate(pid,9),())
        self.assertIn('parent.v0 = child.v0',engine.gates[-1]['equality_columns'])
        self.assertEqual(engine.gates[-1]['gate'],[[0]])
        index[('Mul',5)] = ((5,0,3),)
        self.assertEqual(Matcher(p,index).evaluate(pid,9),((9,0,1,3),))

    def test_no_binding_mixture_and_argument_positions_retained(self):
        p = Patterns()
        pid = p.add(('Add',('Mul',('V','k'),('V','u')),('Mul',('V','k'),('V','v'))))
        index = {('Add',9):((9,4,5),),('Mul',4):((4,0,1),(4,2,3)),
                 ('Mul',5):((5,0,6),(5,2,7))}
        self.assertEqual(Matcher(p,index).evaluate(pid,9),((9,0,1,6),(9,2,3,7)))
        index[('Add',9)] = ((9,5,4),)
        self.assertEqual(Matcher(p,index).evaluate(pid,9),((9,0,6,1),(9,2,7,3)))

    def test_repeated_variable_inside_single_constructor(self):
        p = Patterns()
        pid = p.add(('Mul',('V','x'),('V','x')))
        index = {('Mul',5):((5,0,1),(5,2,2))}
        self.assertEqual(Matcher(p,index).evaluate(pid,5),((5,2),))

    def test_native_rejects_same_shapes_with_different_common_factor(self):
        patterns = Patterns.from_ast(json.loads((self.out/'rules-ast.json').read_text()))
        seed = ('Add',('Mul',('Input',0),('Input',1)),('Mul',('Input',2),('Input',3)))
        directory = self.out/'negative'
        env = RULES.read_text().replace('(Var String)','(RipenInput i64)')
        native(ROOT/'target/release/examples/native_math_baseline',directory,env+patterns.probe(),
               f'(let root {sexpr(seed)})','(run-schedule (run probe))',
               ['RipenInput','Add','Mul']+[f'Hit{p["id"]}' for p in patterns.nodes])
        graph, family = Graph(directory/'graph.json'), Family()
        root = family.from_graph(graph,graph.value(seed))
        factoring = next(p['id'] for p in patterns.nodes if p['text']=='(Add (Mul v0 v1) (Mul v0 v2))')
        self.assertEqual(Matcher(patterns,class_index(family,[root])).evaluate(factoring,root),())
        self.assertEqual(native_matches(directory/'graph.json',patterns,family)[factoring],set())


if __name__ == '__main__':
    unittest.main()

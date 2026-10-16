"""Native checks and regressions for template/instance accounting."""
from collections import Counter
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from .demo import (ROOT, Family, Graph, SEEDS, ground_key, instantiate,
                   matrix_search, run, small, template_inputs)


class FactorizedBoundary(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix='parameterized-spine-')
        cls.addClassCleanup(cls.temp.cleanup)
        cls.out = Path(cls.temp.name)/'demo'
        cls.report = run(cls.out, ROOT/'target/release/examples/native_math_baseline')

    def test_native_full_closures_and_exact_boundary_bindings(self):
        r = self.report
        self.assertEqual((r['template_count'],r['instance_count']), (2,4))
        self.assertEqual(r['template_candidates'], [12,56])
        self.assertEqual([(s['enodes'],s['eclasses']) for s in r['template_shapes']], [(13,7),(22,11)])
        self.assertEqual(len(r['closure_checks']), 68)
        self.assertTrue(r['complete_native_term_family'])
        self.assertTrue(r['native_binding_equal'])
        self.assertEqual(r['native_distinct_bindings'], 12)
        self.assertEqual(r['native_graft_checks'], 12)
        self.assertEqual(r['scores'], [[8,8,4,0],[8,32,0,0],[4,0,8,0],[0,0,0,32]])

    def test_renamed_instances_do_not_become_new_templates(self):
        old_seeds = [small('x','a','b'),small('x','c','d'),small('y','a','b'),small('z','e','f')]
        templates, instances = template_inputs(old_seeds)
        self.assertEqual(len(templates), 1)
        self.assertEqual(len(instances), 4)
        self.assertEqual({i['template'] for i in instances}, {0})
        once, _ = template_inputs(SEEDS)
        many, bindings = template_inputs(SEEDS*100)
        self.assertEqual(once, many)
        self.assertEqual(len(bindings), 400)

    def test_compact_computation_never_enumerates(self):
        f = Family()
        templates, instances = template_inputs(SEEDS)
        roots = []
        with patch.object(Family, 'enumerate', side_effect=AssertionError('expanded!')), \
             patch.object(Graph, 'enumerate_terms', side_effect=AssertionError('expanded native!')):
            for i, template in enumerate(templates):
                g = Graph(self.out/f'bake/t{i}/graph.json')
                roots.append(f.from_graph(g,g.value(template)))
            projections = [f.root_mul(r) for r in roots]
            result = matrix_search([instantiate(projections[i['template']],i['binding']) for i in instances])
        self.assertEqual(result['scores'], self.report['scores'])
        self.assertTrue(all(node[0] in ('Input','Add','Mul','Or') for node in f.nodes))
        self.assertIn(roots[0],f.reachable(roots[1]))
        self.assertEqual(len(f.nodes),29)

    def test_existing_ripsat_sharing_is_reported(self):
        r = self.report
        self.assertTrue(r['structural_sharing_same_as_ripsat'])
        self.assertTrue(r['smaller_template_is_subcircuit'])
        s = r['storage']
        self.assertEqual((s['ripsat_enodes'],s['ripsat_eclasses']),(22,11))
        self.assertLess(s['shared_factorized_nodes'],s['independent_factorized_nodes'])
        self.assertGreater(s['binding_bytes'],0)
        self.assertGreater(s['compiled_projection_bytes'],0)
        self.assertEqual(r['symbolic_projection_compilations'],2)

    def test_correlated_binding_is_not_replaced_by_cartesian_product(self):
        result = matrix_search([Counter({('x','u'):2,('y','v'):3}),Counter({('x','w'):5})])
        matches = [r for r in result['records'] if (r['left'],r['right']) == (0,1)]
        self.assertEqual([(r['factor'],r['u'],r['v']) for r in matches],[('x','u','w')])
        self.assertEqual(result['scores'][0][1],10)

    def test_alias_and_uncertified_interface_domain_are_explicit(self):
        with self.assertRaisesRegex(ValueError,'aliased inputs'):
            instantiate(Counter(),['x','x'])
        with self.assertRaisesRegex(ValueError,'outside certified'):
            ground_key(('Mul',('Var','x'),('Var','y')))
        self.assertEqual(ground_key(('Add',('Var','x'),('Var','y'))),
                         ground_key(('Add',('Var','y'),('Var','x'))))

    def test_empty_boundary_projection(self):
        self.assertEqual(matrix_search([Counter(),Counter()])['scores'],[[0,0],[0,0]])

    def test_view_distinguishes_templates_and_instances(self):
        page = (self.out/'index.html').read_text()
        for text in ['一个模板的四个实例','RipenInput','I0/T0','I1/T1','直接共享 RipSat egraph','binding']:
            self.assertIn(text,page)


if __name__ == '__main__':
    unittest.main()

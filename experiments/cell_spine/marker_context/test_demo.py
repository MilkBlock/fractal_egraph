"""Shallow markers preserve structure, expose costs, and update incrementally."""
from contextlib import ExitStack
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from . import demo
from experiments.cell_spine.factorized_boundary import demo as old
from experiments.cell_spine.shared_matching.demo import Matcher


class MarkerContext(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory(prefix='marker-context-')
        cls.addClassCleanup(cls.temp.cleanup)
        cls.out=Path(cls.temp.name)/'run'
        cls.report=demo.run(cls.out,skip_build=True)
        cls.library=json.loads((cls.out/'bake/library.json').read_text())
        cls.prepared=demo.layers.prepare(cls.library,cls.out/'bake')
        cls.book=demo.tensor.Codebook(demo.tensor.ALPHABET)
        cls.base=[cls.book.encode(t,b,cls.library['templates'][t]['arity'],4) for t,b in demo.tensor.CASES]

    def marked(self,policy):
        plan=demo.MarkerPlan(self.library,policy)
        return plan,[plan.build(c)[0] for c in self.base]

    def test_native_bindings_and_full_cell_append(self):
        r=self.report
        self.assertTrue(r['native_full_binding_equal'])
        self.assertEqual((r['matches'],r['global_rule_bindings']),(462,110))
        self.assertEqual(r['append_native_closure'],dict(exact_equal=True,enodes=22,eclasses=11))

    def test_matching_and_cold_construction_costs(self):
        r=self.report
        self.assertEqual(r['baseline']['root']['stats']['multiply_adds'],1568)
        self.assertEqual(r['policies']['pair']['modes']['root']['stats']['multiply_adds'],1088)
        self.assertEqual(r['policies']['whole']['modes']['root']['stats']['multiply_adds'],1136)
        self.assertEqual(r['policies']['pair']['construction']['marker_scalar_adds'],128)
        self.assertEqual(r['policies']['whole']['construction']['marker_scalar_adds'],192)
        self.assertEqual(r['policies']['pair']['construction']['stored_extra_entries'],128)
        self.assertEqual(r['policies']['whole']['construction']['stored_extra_entries'],256)

    def test_scheduler_change_not_attributed_to_markers(self):
        r=self.report
        self.assertEqual(r['legacy_baseline']['all']['stats']['multiply_adds'],9504)
        fair=r['baseline']['all']['stats']['multiply_adds']
        self.assertEqual(fair,8736)
        for p in r['policies'].values():
            self.assertEqual(p['modes']['all']['stats']['multiply_adds'],fair)
            self.assertGreater(p['modes']['all']['stats']['marker_equalities_inferred'],0)

    def test_same_bag_wrong_parentheses_remains_unresolved_then_rejects(self):
        plan,cells=self.marked('whole')
        r=demo.marker_score(self.library,self.prepared,cells,plan,'root')
        def compound(instance):
            return next(i for i,c in enumerate(r['channels']) if c['instance']==instance and
                        self.library['recipes'][c['recipe']]['shape']=='A(A(I,I),I)')
        a,b=compound(1),compound(6)
        i=a*len(r['channels'])+b
        self.assertEqual(cells[1].ports[5],cells[6].ports[5])
        self.assertEqual([s[i] for s in r['state_snapshots']],['continue','continue','rejected'])
        self.assertFalse(r['gate'][a][b])

    def test_pair_marker_is_exact_and_symmetric(self):
        plan,cells=self.marked('pair')
        self.assertEqual(cells[0].ports[4],cells[4].ports[4])
        for i,a in enumerate(cells):
            for j,b in enumerate(cells):
                dot=sum(x*y for x,y in zip(a.ports[4],b.ports[4]))
                expected=set(demo.tensor.CASES[i][1][1:3])==set(demo.tensor.CASES[j][1][1:3])
                self.assertEqual(dot==32,expected)

    def test_same_inner_pair_different_outer_requires_remaining_guard(self):
        base=[self.book.encode(1,b,4,4) for b in [['x','a','b','c'],['y','a','b','d']]]
        outputs={}
        for policy in ('pair','whole'):
            plan=demo.MarkerPlan(self.library,policy)
            cells=[plan.build(c)[0] for c in base]
            r=demo.marker_score(self.library,self.prepared,cells,plan,'root')
            pair=[next(i for i,c in enumerate(r['channels']) if c['instance']==instance and
                       self.library['recipes'][c['recipe']]['shape']=='A(A(I,I),I)') for instance in (0,1)]
            job=pair[0]*len(r['channels'])+pair[1]
            states=[s[job] for s in r['state_snapshots']]
            self.assertEqual(states[0],'continue')
            self.assertEqual(states[-1],'rejected')
            self.assertEqual(states[1],'continue' if policy=='pair' else 'rejected')
            outputs[policy]=r['gate']
        self.assertEqual(outputs['pair'],outputs['whole'])

    def test_incremental_append_and_dependencies(self):
        for policy,adds in [('pair',0),('whole',16)]:
            plan=demo.MarkerPlan(self.library,policy)
            source,_=plan.build(self.base[0])
            new,cost=plan.update(source,1,{3:tuple(self.book.h[self.book.labels.index('c')])})
            fresh,_=plan.build(self.base[1])
            self.assertEqual(new,fresh)
            self.assertIs(new.ports[4],source.ports[4])
            self.assertEqual(cost.get('marker_scalar_adds',0),adds)
            changed,cost=plan.update(new,1,{0:tuple(self.book.h[self.book.labels.index('z')])})
            self.assertEqual(cost.get('marker_scalar_adds',0),0)
            self.assertIs(changed.ports[4],new.ports[4])

    def test_no_constructor_query_or_matcher_for_marker_build_update_or_score(self):
        plan=demo.MarkerPlan(self.library,'pair')
        with ExitStack() as stack:
            for obj,name in [(old.Graph,'value'),(old.Graph,'representative'),(old.Family,'root_mul'),
                             (old.Family,'from_graph'),(old.Family,'enumerate'),
                             (old,'ground_key'),(old,'matrix_search'),(Matcher,'evaluate'),
                             (demo.tensor.Codebook,'decode'),(demo.tensor,'score')]:
                stack.enter_context(patch.object(obj,name,side_effect=AssertionError('forbidden '+name)))
            cells=[plan.build(c)[0] for c in self.base]
            plan.update(cells[0],1,{3:tuple(self.book.h[self.book.labels.index('c')])})
            result=demo.marker_score(self.library,self.prepared,cells,plan,'all')
        self.assertEqual(sum(map(sum,result['gate'])),462)

    def test_bad_update_is_rejected(self):
        plan,cells=self.marked('whole')
        with self.assertRaisesRegex(ValueError,'aliased'):
            plan.update(cells[0],1,{3:cells[0].ports[0]})
        with self.assertRaisesRegex(ValueError,'unsupported input'):
            plan.update(cells[0],0,{-1:cells[0].ports[0]})
        with self.assertRaisesRegex(ValueError,'codeword'):
            plan.update(cells[0],1,{3:(-1,)*16})


if __name__=='__main__':
    unittest.main()

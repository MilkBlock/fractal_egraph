import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from . import specialization_demo as demo
from .runtime import Runtime


class FactSpecializationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(prefix='fact-specialization-')
        cls.addClassCleanup(cls.tmp.cleanup)
        cls.base=Path(cls.tmp.name)
        cls.report=demo.run(cls.base/'ablation',gpu=False,skip_build=True)
        cls.ast=json.loads((cls.base/'ablation/rules.json').read_text())
        cls.source=(demo.HERE/'rules.egg').read_text()
        cls.binary=demo.ROOT/'target/release/examples/native_math_baseline'
        cls.serial=0

    def runtime(self):
        type(self).serial+=1
        return Runtime(self.base/f'runtime-{self.serial}',self.binary,self.source,self.ast)

    def test_all_variants_match_each_phase_and_native_states(self):
        self.assertTrue(self.report['all_phase_states_equal'])
        for phase in self.report['baseline']['phases']:
            self.assertTrue(phase['native_checks'])
            self.assertTrue(all(c['exact_equal'] for c in phase['native_checks']))

    def test_fresh_names_and_refined_facts_skip_matrix_work(self):
        for i in (1,3):
            baseline=self.report['baseline']['phases'][i]['metrics']
            learned=self.report['specialized']['phases'][i]['metrics']
            self.assertGreater(baseline['matrix_batches'],0)
            self.assertEqual(learned['matrix_batches'],0)
            self.assertEqual(learned['native_bakes'],0)
            self.assertEqual(learned['fact_replays'],13)
        changed=self.report['specialized']['phases'][2]['metrics']
        self.assertGreater(changed['certificates'],0)
        self.assertGreater(changed['matrix_batches'],0)

    def test_cold_proof_cost_and_direct_fact_ablation_are_exposed(self):
        base=self.report['baseline']['phases'][0]['metrics']
        learned=self.report['specialized']['phases'][0]['metrics']
        direct=self.report['direct_fact']['phases'][0]['metrics']
        self.assertGreater(learned['certificate_native_bakes'],0)
        self.assertGreaterEqual(learned['native_bakes'],base['native_bakes'])
        self.assertLess(direct['native_bakes'],base['native_bakes'])
        self.assertEqual(direct['matrix_batches'],0)

    def test_unobserved_fact_does_not_create_a_recipe(self):
        rt=self.runtime();pair=('Add',('Input',0),('Input',1));m=('Mul',pair,('Input',2))
        a=rt.seed(m,[0,1,2]);b=rt.seed(m,[0,1,3])
        self.assertEqual(rt.specializations.entries,[])
        root=rt.construct('Add',a,b)
        self.assertEqual(rt.specializations.entries,[])
        rt.drain();self.assertIsNotNone(rt.resolve(root))
        self.assertEqual(len(rt.specializations.entries),1)

    def test_warm_path_needs_no_matcher_native_bake_or_vector_encoding(self):
        rt=self.runtime();pair=('Add',('Input',0),('Input',1));m=('Mul',pair,('Input',2))
        a=rt.seed(m,[0,1,2]);b=rt.seed(m,[0,1,3]);rt.construct('Add',a,b);rt.drain()
        a=rt.seed(m,[100,101,102]);b=rt.seed(m,[100,101,103]);root=rt.construct('Add',a,b)
        with patch.object(rt,'process',side_effect=AssertionError('matcher used')), \
             patch.object(rt.library,'add',side_effect=AssertionError('Bake used')), \
             patch('experiments.cell_spine.transition_program.runtime.bit_code',side_effect=AssertionError('vector encode used')):
            rt.drain()
        self.assertIsNotNone(rt.resolve(root));self.assertEqual(rt.specializations.stats['replays'],1)

    def test_extra_alias_cannot_reuse_the_old_guard(self):
        rt=self.runtime();k=('Add',('Input',0),('Input',1));p=('Pow',k,('Input',2))
        a=rt.seed(p,[0,1,2]);b=rt.seed(p,[0,1,3]);rt.construct('Mul',a,b);rt.drain()
        fresh=rt.seed(p,[10,11,12]);h=rt.resolve(fresh)
        self.assertIsNone(rt.specializations.apply('Mul',h,h))
        rt.construct('Mul',fresh,fresh);rt.drain()
        self.assertGreater(len(rt.specializations.entries),1)
        self.assertTrue(all(e['groups_must_remain_distinct'] for e in rt.specializations.entries))

    def test_root_equal_but_effect_incomplete_result_is_not_certified(self):
        rt=self.runtime();k=('Add',('Input',0),('Input',1));p=('Pow',k,('Input',2))
        a=rt.seed(p,[0,1,2]);b=rt.seed(p,[0,1,3])
        rhs=rt.seed(('Pow',k,('Add',('Input',2),('Input',3))),[0,1,2,3])
        with self.assertRaisesRegex(AssertionError,'full native fact-context closure'):
            rt.specializations.observe('Mul',rt.resolve(a),rt.resolve(b),rt.resolve(rhs))
        self.assertEqual(rt.specializations.entries,[])

    def test_changed_environment_invalidates_the_transfer_contract(self):
        rt=self.runtime();a=rt.seed(('Input',0),[0]);h=rt.resolve(a)
        rt.library.source+='\n; changed environment\n'
        with self.assertRaisesRegex(ValueError,'environment changed'):
            rt.specializations.apply('Add',h,h)

    def test_no_concrete_identity_is_stored_in_the_recipe(self):
        recipes=self.report['specialized']['summary']['fact_specializations']['recipes']
        self.assertTrue(recipes)
        for r in recipes:
            self.assertTrue(r['proof']['native_rooted_whole_closure_equal'])
            self.assertEqual(len(r['equality_partition']),sum(r['source_arities']))
            self.assertTrue(all(0<=i<sum(r['source_arities']) for i in r['output_sources']))
            self.assertNotIn('actual_binding',r)
            self.assertNotIn('instance_ids',r)

    def test_report_does_not_change_after_later_replays(self):
        rt=self.runtime();k=('Add',('Input',0),('Input',1));m=('Mul',k,('Input',2))
        a=rt.seed(m,[0,1,2]);b=rt.seed(m,[0,1,3]);rt.construct('Add',a,b);rt.drain()
        old=rt.summary()
        a=rt.seed(m,[10,11,12]);b=rt.seed(m,[10,11,13]);rt.construct('Add',a,b);rt.drain()
        self.assertEqual(old['fact_specializations']['recipes'][0]['replays'],0)
        self.assertEqual(rt.summary()['fact_specializations']['recipes'][0]['replays'],1)


if __name__=='__main__':unittest.main()

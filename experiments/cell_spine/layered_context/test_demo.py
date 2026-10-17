"""Depth frontier correctness, full-binding preservation, and fair work counters."""
from contextlib import ExitStack
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from .demo import run, staged, EqualityBatch, classify
from experiments.cell_spine.tensor_interaction import demo as tensor
from experiments.cell_spine.factorized_boundary import demo as old
from experiments.cell_spine.shared_matching.demo import Matcher


class LayeredContext(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory(prefix='layered-context-')
        cls.addClassCleanup(cls.temp.cleanup)
        cls.out=Path(cls.temp.name)/'run'
        cls.report=run(cls.out,skip_build=True)
        cls.library=json.loads((cls.out/'bake/library.json').read_text())
        cls.prepared=json.loads((cls.out/'prepared.json').read_text())
        cls.prepared['depths']={int(k):v for k,v in cls.prepared['depths'].items()}
        book=tensor.Codebook(tensor.ALPHABET)
        cls.cells=[book.encode(t,b,cls.library['templates'][t]['arity'],4) for t,b in tensor.CASES]

    def test_native_and_eager_equivalence(self):
        r=self.report
        self.assertTrue(r['native_full_binding_equal'])
        self.assertTrue(r['eager_equal'])
        self.assertTrue(r['legacy_tensor_equal'])
        self.assertEqual((r['matches'],r['global_rule_bindings']),(462,110))
        self.assertEqual(r['studies']['root']['matches'],38)
        self.assertEqual(r['studies']['all']['context'],dict(offered_contexts=5184,
                         constructor_rejected=4160,constructor_continued=1024))

    def test_unknown_survives_and_deeper_evidence_completes(self):
        s=self.report['studies']['root']
        self.assertEqual([x['continue'] for x in s['trace']],[80,24,2,0])
        self.assertEqual([x['complete'] for x in s['trace']],[16,30,36,38])
        total=s['channels']**2
        for x in s['trace']:
            self.assertEqual(x['complete']+x['rejected']+x['continue'],total)
        for before,after in zip(s['status_snapshots'],s['status_snapshots'][1:]):
            self.assertTrue(all(a=='continue' or a==b for a,b in zip(before,after)))

    def test_partial_compound_context_is_not_false(self):
        r=staged(self.library,self.prepared,self.cells,'root')
        def compound(instance):
            return next(i for i,c in enumerate(r['channels']) if c['instance']==instance and
                        self.library['recipes'][c['recipe']]['shape']=='A(A(I,I),I)')
        n=len(r['channels'])
        good=compound(3)*n+compound(5)  # same outer f; swapped inner d/e
        bad=compound(1)*n+compound(6)   # wrong outer leaf, despite same leaf set
        self.assertEqual([s[good] for s in r['state_snapshots']],['continue','continue','continue','complete'])
        self.assertEqual([s[bad] for s in r['state_snapshots']],['continue','continue','rejected','rejected'])

    def test_unknown_is_not_an_absent_fact(self):
        e=EqualityBatch(self.cells,4)
        job=dict(alternatives=(((2,1,5),),))
        self.assertIsNone(e.known(1,5))
        state,_=classify(job,e)
        self.assertEqual(state,'continue')
        e.resolve([(1,5)],2)
        self.assertEqual(classify(job,e)[0],'complete')

    def test_matrix_savings_and_extra_batches_both_reported(self):
        root=self.report['studies']['root']
        self.assertEqual(root['eager']['multiply_adds'],3232)
        self.assertEqual(root['layered']['multiply_adds'],1568)
        self.assertGreater(root['layered']['matrix_calls'],root['eager']['matrix_calls'])
        all_=self.report['studies']['all']
        self.assertEqual(all_['layered']['multiply_adds'],all_['eager']['multiply_adds'])
        self.assertEqual(all_['layered']['multiply_adds'],9504)
        self.assertGreater(root['layered']['speculative_matrix_cells'],0)
        self.assertGreater(root['layered']['state_classifications'],0)
        self.assertGreater(root['layered']['evidence_reads'],0)

    def test_no_hidden_matcher_or_new_binding_keys(self):
        with ExitStack() as stack:
            for obj,name in [(old.Graph,'value'),(old.Graph,'representative'),
                             (old.Family,'from_graph'),(old.Family,'root_mul'),
                             (old.Family,'enumerate'),(old,'ground_key'),(old,'matrix_search'),
                             (Matcher,'evaluate'),(tensor,'score'),(tensor,'term_recipe'),
                             (tensor.Codebook,'decode')]:
                stack.enter_context(patch.object(obj,name,side_effect=AssertionError('forbidden '+name)))
            result=staged(self.library,self.prepared,self.cells,'all')
        self.assertEqual(sum(map(sum,result['gate'])),462)

    def test_batches_share_relative_wiring_and_count_full_dense_work(self):
        for study in self.report['studies'].values():
            self.assertEqual(study['layered']['multiply_adds'],
                             sum(b['shape'][0]*b['shape'][1]*b['shape'][2] for b in study['blocks']))
            for b in study['blocks']:
                self.assertEqual(b['shape'][1],16)
                self.assertTrue(all(p%4==b['slots'][0] for p in b['left_sources']))
                self.assertTrue(all(q%4==b['slots'][1] for q in b['right_sources']))


if __name__=='__main__':
    unittest.main()

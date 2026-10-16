"""Forbid query/key preparation in the numerical interaction kernel."""
from contextlib import ExitStack
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from . import demo
from experiments.cell_spine.factorized_boundary import demo as old
from experiments.cell_spine.shared_matching.demo import Matcher


class TensorInteraction(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory(prefix='tensor-spine-')
        cls.addClassCleanup(cls.temp.cleanup)
        cls.out=Path(cls.temp.name)/'run'
        with (patch.object(old.Family,'enumerate',side_effect=AssertionError('expanded candidates')),
              patch.object(old.Graph,'enumerate_terms',side_effect=AssertionError('expanded graph'))):
            cls.report=demo.run(cls.out,skip_build=True)
        cls.library=json.loads((cls.out/'bake/library.json').read_text())
        cls.book=demo.Codebook(demo.ALPHABET)
        cls.cells=[cls.book.encode(t,b,cls.library['templates'][t]['arity'],cls.library['width'])
                   for t,b in demo.CASES]

    def test_exact_native_bindings_all_anchor_regions(self):
        r=self.report
        self.assertEqual((r['port_count'],r['channel_count']),(32,64))
        self.assertEqual(r['matches'],462)
        self.assertEqual(r['original_rule_target_checks'],462)
        self.assertTrue(r['native_full_binding_equal'])
        self.assertTrue(r['dense_tensor_equal'])
        self.assertEqual(r['matches_by_location'],{'root/root':38,'root/non-root':71,
                                                  'non-root/root':71,'non-root/non-root':282})

    def test_kernel_has_no_old_matcher_no_constructor_queries_no_decoding(self):
        with ExitStack() as stack:
            for obj,name in [(old.Family,'root_mul'),(old.Family,'from_graph'),
                             (old.Graph,'value'),(old.Graph,'representative'),
                             (old,'matrix_search'),(old,'instantiate'),
                             (Matcher,'evaluate'),(demo.Codebook,'decode'),
                             (demo,'ground_key'),(demo,'term_recipe'),(demo,'dense_feature')]:
                stack.enter_context(patch.object(obj,name,side_effect=AssertionError('forbidden '+name)))
            result=demo.score(self.library,self.cells)
        self.assertEqual(sum(map(sum,result['gate'])),462)
        self.assertEqual(result['stats']['constructor_queries'],0)
        self.assertEqual(result['stats']['tuple_keys_constructed'],0)
        self.assertEqual(result['stats']['gram_calls'],1)

    def test_full_encoding_roundtrip_without_partner(self):
        self.assertEqual(set(demo.EncodedCell.__dataclass_fields__),{'template','ports'})
        for c,(t,b) in zip(self.cells,demo.CASES):
            self.assertEqual(self.book.decode(c,self.library['templates'][t]['arity']),b)
            self.assertEqual(len(c.ports),4)
            self.assertTrue(all(v in (-1,0,1) for row in c.ports for v in row))

    def test_codebook_is_exact_not_random_similarity(self):
        g=demo.mm(self.book.h,demo.transpose(self.book.h))
        self.assertEqual(g,[[16*int(i==j) for j in range(16)] for i in range(16)])

    def test_commutativity_without_spurious_associativity(self):
        r=self.report['root_matrix']
        self.assertEqual(r[0],r[4])
        self.assertEqual(r[3],r[5])
        # Same a,b,c leaves, different grouping; different outer factors too.
        self.assertEqual(r[1][6],0)

    def test_unseen_bindings_reuse_frozen_library(self):
        before=json.dumps(self.library,sort_keys=True)
        fresh=[(0,['g','h','i']),(1,['g','h','i','j']),
               (0,['k','h','i']),(1,['m','h','j','i'])]
        cells=[self.book.encode(t,b,self.library['templates'][t]['arity'],4) for t,b in fresh]
        with patch.object(demo,'bake',side_effect=AssertionError('rebaked')):
            result=demo.score(self.library,cells)
            directory=self.out/'unseen'
            directory.mkdir()
            records=demo.native_oracle(directory,demo.ROOT/'target/release/examples/native_math_baseline',
                                       self.library,cells,self.book,result)
        self.assertTrue(records)
        self.assertEqual(before,json.dumps(self.library,sort_keys=True))

    def test_invalid_domain_is_rejected(self):
        with self.assertRaisesRegex(ValueError,'aliased'):
            self.book.encode(0,['x','a','a'],3,4)
        with self.assertRaises(ValueError):
            self.book.encode(0,['unknown','a','b'],3,4)
        with self.assertRaisesRegex(ValueError,'no vector codec'):
            demo.term_recipe(('Mul',('Input',0),('Input',1)))
        with self.assertRaisesRegex(ValueError,'repeated leaves'):
            demo.term_recipe(('Add',('Input',0),('Input',0)))


if __name__=='__main__':
    unittest.main()

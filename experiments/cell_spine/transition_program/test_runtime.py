import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from . import adaptive_demo as demo
from .runtime import Runtime,closure_key,validate_environment
from .compiler import minimum_terms


class AdaptiveRuntime(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(prefix='adaptive-cells-')
        cls.addClassCleanup(cls.tmp.cleanup)
        cls.base=Path(cls.tmp.name);cls.out=cls.base/'demo'
        cls.report=demo.run(cls.out,gpu=False,skip_build=True)
        cls.ast=json.loads((cls.out/'rules.json').read_text())
        cls.source=(demo.HERE/'rules.egg').read_text()
        cls.binary=demo.ROOT/'target/release/examples/native_math_baseline'
        cls.serial=0

    def runtime(self,source=None,ast=None):
        type(self).serial+=1
        return Runtime(self.base/f'case-{self.serial}',self.binary,source or self.source,ast or self.ast)

    def test_alias_repaired_and_followers_resume(self):
        r=self.report['cpu']
        self.assertGreater(r['after']['runtime']['alias_targets_resumed'],0)
        self.assertGreater(r['after']['library']['alias_cache_hits'],0)
        self.assertEqual(r['after']['constructor_facts'],r['after']['resolved_facts'])

    def test_full_native_graphs_before_union_after_union_and_late_fact(self):
        for phase,checks in self.report['cpu']['checks'].items():
            self.assertTrue(checks,phase)
            self.assertTrue(all(c['exact_equal'] for c in checks),phase)

    def test_depth_not_fixed_to_two_and_updates_are_local(self):
        r=self.report['cpu']
        self.assertGreater(len(r['before']['batches']),2)
        self.assertEqual(r['union_touched_nodes'],[0,1,2,3,4])
        self.assertTrue(r['independent_untouched'])
        self.assertTrue(r['quiescent_repeat_no_work'])

    def test_cycle_reuses_saturated_state_without_erasing_trigger_effects(self):
        cycle=self.report['cpu']['cycle_cells']
        self.assertEqual(cycle[1],cycle[3]);self.assertEqual(cycle[3],cycle[5])
        self.assertEqual(cycle[2],cycle[4])
        self.assertNotEqual(cycle[0],cycle[2], 'same Sin root but different retained closures')

    def test_symmetric_bindings_intern_but_rhs_only_does_not(self):
        rt=self.runtime();pair=('Add',('Input',0),('Input',1));m=('Mul',pair,('Input',2))
        a=rt.seed(m,[0,1,2]);b=rt.seed(m,[1,0,2])
        self.assertEqual(rt.resolve(a),rt.resolve(b))
        coarse=('Mul',('Pow',pair,('Input',2)),('Pow',pair,('Input',3)))
        rhs=('Pow',pair,('Add',('Input',2),('Input',3)))
        c=rt.seed(coarse,[0,1,2,3]);d=rt.seed(rhs,[0,1,2,3])
        self.assertNotEqual(rt.resolve(c),rt.resolve(d))
        cr,dr=rt.cells.get(rt.resolve(c)),rt.cells.get(rt.resolve(d))
        self.assertEqual(cr['key'][0],dr['key'][0], 'same root representative')
        self.assertNotEqual(cr['key'][1],dr['key'][1], 'retained rows differ')

    def test_alias_variants_shared_across_values(self):
        rt=self.runtime();seed=('Mul',('Add',('Input',0),('Input',1)),('Add',('Input',2),('Input',3)))
        a=rt.seed(seed,[0,1,2,2]);b=rt.seed(seed,[10,11,12,12])
        ca,cb=rt.cells.get(rt.resolve(a)),rt.cells.get(rt.resolve(b))
        self.assertEqual(ca['template'],cb['template'])
        self.assertEqual(len(ca['binding']),3)
        self.assertEqual(rt.library.stats['alias_variants_baked'],1)
        self.assertGreater(rt.library.stats['alias_cache_hits'],0)

    def test_no_match_still_retains_constructor_fact(self):
        r=self.report['cpu']['after']['runtime']
        self.assertGreater(r['no_match_contexts_retained'],0)
        self.assertGreater(r['native_context_fallbacks'],0)

    def test_dynamic_bit_width_and_slot_count(self):
        self.assertGreater(max(b['slots'] for b in self.report['cpu']['after']['batches']),5)
        rt=self.runtime();m=('Mul',('Add',('Input',0),('Input',1)),('Input',2))
        a=rt.seed(m,[1<<32,(1<<32)+1,7]);b=rt.seed(m,[(1<<32)+1,1<<32,8])
        root=rt.construct('Add',a,b);rt.drain()
        self.assertIsNotNone(rt.resolve(root));self.assertEqual(rt.batches[0]['bits'],33)
        self.assertLess(rt.batches[0]['feature_rows_built'],rt.batches[0]['temporary_rows'])

    def test_new_rule_is_discovered_from_ast_not_four_name_whitelist(self):
        source=self.source+'\n(rewrite (Add (Sin x) (Cos x)) (Sin x) :name "sin-cos-absorb")\n'
        path=self.base/'extra.egg';path.write_text(source)
        ast_path=self.base/'extra.json'
        subprocess.run([demo.ROOT/'target/release/examples/lhs_ast',path,ast_path],check=True)
        ast=json.loads(ast_path.read_text());rt=self.runtime(source,ast)
        self.assertIn('sin-cos-absorb',rt.rules)
        pair=('Add',('Input',0),('Input',1))
        left=rt.seed(('Sin',pair),[0,1]);right=rt.seed(('Cos',pair),[1,0])
        root=rt.construct('Add',left,right);rt.drain()
        self.assertIsNotNone(rt.resolve(root))
        self.assertEqual(rt.stats['native_context_fallbacks'],0)
        checks=demo.audit(rt,self.base/'extra-audit',self.binary,source)
        self.assertTrue(all(c['exact_equal'] for c in checks))

    def test_repeated_unions_retarget_old_handles(self):
        rt=self.runtime();m=('Mul',('Add',('Input',0),('Input',1)),('Input',2))
        a=rt.seed(m,[0,1,2]);b=rt.seed(m,[0,3,2]);c=rt.seed(m,[0,4,2])
        root=rt.construct('Add',rt.construct('Add',a,b),c);rt.drain()
        rt.union_inputs(1,3);rt.drain();rt.union_inputs(1,4);rt.drain()
        self.assertEqual(rt.resolve(a),rt.resolve(b));self.assertEqual(rt.resolve(b),rt.resolve(c))
        self.assertIsNotNone(rt.resolve(root))
        checks=demo.audit(rt,self.base/'union-audit',self.binary,self.source)
        self.assertTrue(all(c['exact_equal'] for c in checks))

    def test_failed_batch_does_not_get_marked_complete(self):
        rt=self.runtime();m=('Mul',('Add',('Input',0),('Input',1)),('Input',2))
        a=rt.seed(m,[0,1,2]);b=rt.seed(m,[0,1,3]);root=rt.construct('Add',a,b)
        with patch.object(rt,'process',side_effect=RuntimeError('test interruption')):
            with self.assertRaises(RuntimeError):rt.drain()
        self.assertIsNone(rt.resolve(root));self.assertTrue(rt.queue)
        rt.drain();self.assertIsNotNone(rt.resolve(root))

    def test_invalid_dag_reference_rejected(self):
        rt=self.runtime();a=rt.seed(('Input',0),[0])
        with self.assertRaises(ValueError):rt.construct('Add',a,('node',100))

    def test_guards_and_literal_data_are_not_silently_erased(self):
        import copy
        guarded=copy.deepcopy(self.ast);guarded['rewrites'][0]['conditions']=[{'some':'guard'}]
        with self.assertRaisesRegex(ValueError,'separate Cell contract'):validate_environment(guarded)
        literal=copy.deepcopy(self.ast)
        literal['rewrites'][0]['rhs']={'kind':'literal','value':1}
        with self.assertRaisesRegex(ValueError,'erasure'):validate_environment(literal)


if __name__=='__main__':unittest.main()

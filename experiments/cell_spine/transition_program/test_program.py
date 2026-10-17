import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from . import demo
from .compiler import Program,Bake,minimum_terms
from experiments.cell_spine.factorized_boundary.demo import Graph
from experiments.cell_spine.moment_compiler.compiler import bit_code


class TransitionProgram(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(prefix='transition-program-')
        cls.addClassCleanup(cls.tmp.cleanup)
        cls.out=Path(cls.tmp.name)/'run'
        cls.report=demo.run(cls.out,gpu=False,skip_build=True)
        cls.payload=json.loads((cls.out/'program.json').read_text())
        cls.library=json.loads((cls.out/'library.json').read_text())

    def test_four_rules_and_complete_binding_oracle(self):
        self.assertEqual(set(self.report['source_rules']),{'factor','powers','diff-sin','integral-cos'})
        self.assertEqual([s['native_full_bindings'] for s in self.report['native']['matches']],[6,4])
        self.assertTrue(all(s['exact'] for s in self.report['native']['matches']))

    def test_all_accepted_targets_preserve_entire_closure(self):
        checks=self.report['native']['full_closure_checks']
        self.assertEqual(len(checks),8)
        self.assertTrue(all(c['exact_equal'] for c in checks))
        counts={c['label']:c['enodes'] for c in checks}
        self.assertEqual(counts['factor-again'],63)
        self.assertEqual(counts['powers-again'],20)

    def test_rhs_only_loses_retained_facts(self):
        negative=self.report['native']['rhs_only_negative_ablation'][0]
        self.assertFalse(negative['exact_equal'])
        self.assertLess(negative['rhs_only_enodes'],negative['full_enodes'])

    def test_generated_cell_is_consumed_by_different_rule(self):
        details=self.report['transitions']
        first={t['target'] for t in details if t['stage']==0 and t['status']==1}
        second=[t for t in details if t['stage']==1 and t['status']==1]
        self.assertEqual(len(second),4)
        self.assertTrue(all(first.intersection(t['source_cells']) for t in second))
        self.assertEqual({t['rule'] for t in second},{'factor','powers','diff-sin','integral-cos'})

    def test_target_alias_is_pending_not_mismatch_or_valid(self):
        pending=[t for t in self.report['transitions'] if t['status']==2]
        self.assertEqual(len(pending),2)
        self.assertTrue(all(t['label']=='alias-factor' for t in pending))
        blocked=[t for t in self.report['transitions'] if t['label']=='blocked-after-alias']
        self.assertTrue(blocked)
        self.assertTrue(all(t['status']==0 for t in blocked))

    def test_failed_sources_cannot_be_reactivated_by_padding(self):
        blocked=[t for t in self.report['transitions'] if t['label']=='blocked-after-mismatch']
        self.assertTrue(blocked)
        self.assertTrue(all(t['status']==0 for t in blocked))

    def test_shared_conditions_are_used_across_rules(self):
        self.assertEqual([s['shared_matrices'] for s in self.report['stages']],[2,1])
        for job in self.payload['jobs']:
            usage={};rules={}
            by_branch={x['branch']:x['rule'] for x in self.report['transitions']}
            for t in job['transitions']:
                for query,_ in t['conditions']:
                    usage[query]=usage.get(query,0)+1
                    rules.setdefault(query,set()).add(by_branch[t['branch']])
            self.assertGreater(max(usage.values()),1)
            self.assertEqual(max(map(len,rules.values())),4)

    def test_new_bindings_reuse_program_and_flip_successful_path(self):
        h=self.report['heldout']
        self.assertTrue(h['same_compiled_program'])
        self.assertTrue(all(x['exact'] for x in h['native']['matches']))
        self.assertTrue(all(x['exact_equal'] for x in h['native']['full_closure_checks']))
        original=[[t['status'] for t in self.report['transitions'] if t['stage']==i] for i in (0,1)]
        self.assertNotEqual(original[0],h['statuses'][0])
        self.assertNotEqual(original[1],h['statuses'][1])

    def test_source_target_slots_form_acyclic_stages(self):
        available={i for i,v in enumerate(self.payload['active']) if v}
        for job in self.payload['jobs']:
            targets={t['target'] for t in job['transitions']}
            self.assertTrue(targets.isdisjoint(available))
            self.assertEqual(len(targets),len(job['transitions']))
            for t in job['transitions']:
                self.assertTrue(set(t['sources'])<=available)
                self.assertTrue(all(i in t['sources'] for i,_ in t['bindings']))
            available.update(targets)

    def test_cyclic_saturated_cell_has_finite_representatives(self):
        # Integral/Cos and Diff/Sin reconnect the old ground Cos/Sin classes.
        # Select by source, not an implementation-dependent branch ordinal.
        candidates=[]
        for path in (self.out/'closures').glob('*/source/graph.json'):
            g=Graph(path)
            if {'Diff','Integral'}<={r['op'] for r in g.rows}:candidates.append(g)
        self.assertTrue(candidates)
        for g in candidates:self.assertEqual(set(minimum_terms(g)),set(g.by_class))


if __name__=='__main__':unittest.main()

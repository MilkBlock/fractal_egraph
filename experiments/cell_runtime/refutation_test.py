"""Marker-anchored negative receipts: exact off/on stages and LHS bindings."""
import json, os, tempfile, unittest
from pathlib import Path
from eggfuse.engine.cell_runtime import prepare, execute
from eggfuse.ir.cell_runtime import build
from experiments.cell_runtime.audit import check as audit
from experiments.cell_runtime.reference_views_test import FRONTEND, ENGINE, norm

DECL = '''(datatype E (N i64) (P E E) (R E E) (Trigger E i64) (Seen E i64))
(function View (E i64) Unit :merge old)
(ruleset consume) (ruleset change) (ruleset extra) (ruleset hide) (ruleset make)
(rule ((View x t) (P x y) (R y z)) ((Seen x t)) :ruleset consume)
'''
FACTS = '''(P (N 0) (N 1)) (P (N 0) (N 2))
(R (N 3) (N 4)) (R (N 4) (N 3))
(set (View (N 0) 0) ()) (set (View (N 0) 1) ())
'''

class RefutationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
    def tearDown(self): self.temp.cleanup()
    def pair(self, source, extra=None):
        src = self.root/'source.egg'; src.write_text(source)
        p = build(prepare(src, FRONTEND), reference_tables=['View'])
        reports = []
        for mode in ('0','1'):
            reports.append(execute(p, self.root/mode, ENGINE, audit=True,
                environment={'CELL_CPU_MARKER_REFUTATION':mode, 'CELL_CPU_SIEVE':'0',
                    'CELL_CPU_REFERENCE_VIEWS':'1','CELL_CPU_COMPOSITIONS':'0',
                    **(extra or {})}))
        for name in ('stages.json','binding-audit.json','state.json'):
            a,b=[json.loads((self.root/m/'runtime'/name).read_text()) for m in ('0','1')]
            self.assertEqual(a,b,name)
        self.assertEqual(audit(p,self.root/'0/runtime'), audit(p,self.root/'1/runtime'))
        return reports
    def test_reuses_complete_negative_across_marker_instances(self):
        a,b=self.pair(DECL+FACTS+'(run-schedule (run consume))')
        s=b['marker_refutation']['stats']
        self.assertGreater(s['admitted'],0); self.assertGreater(s['hits'],0)
        self.assertLess(b['stats']['residual_candidates'],a['stats']['residual_candidates'])
    def test_alpha_renamed_consumers_share_negative(self):
        source=DECL+'''(rule ((View root index) (P root middle) (R middle last))
  ((Seen last index)) :ruleset consume)
'''+FACTS+'(run-schedule (run consume))'
        _,b=self.pair(source)
        self.assertGreaterEqual(len(b['marker_refutation']['stats']['per_rule_hits']),2)
    def test_repeated_variables_are_not_mixed(self):
        source=DECL.replace('(R y z)', '(R y y)')+'''
(rule ((View root index) (P root middle) (R middle last))
  ((Seen root index)) :ruleset consume)
'''+FACTS+'''(R (N 1) (N 4))
(run-schedule (run consume)) (check (Seen (N 0) 0))'''
        self.pair(source)
    def test_late_fact_invalidates_negative(self):
        _,b=self.pair(DECL+'''(rule () ((R (N 1) (N 4))) :ruleset change)
'''+FACTS+'''(run-schedule (seq (run consume) (run change) (run consume)))
(check (Seen (N 0) 0) (Seen (N 0) 1))''')
        self.assertGreater(b['marker_refutation']['stats']['invalidated'],0)
    def test_union_inside_spine_enables_boundary_match(self):
        _,b=self.pair(DECL+'''(rule () ((union (N 1) (N 3))) :ruleset change)
'''+FACTS+'''(run-schedule (seq (run consume) (run change) (run consume)))
(check (Seen (N 0) 0) (Seen (N 0) 1))''')
        self.assertGreater(b['marker_refutation']['stats']['invalidated'],0)
    def test_unrelated_same_table_update_preserves_receipt(self):
        _,b=self.pair(DECL+'''(rule () ((P (N 99) (N 100)) (set (View (N 0) 2) ())) :ruleset change)
'''+FACTS+'(run-schedule (seq (run consume) (run change) (run consume)))')
        self.assertGreaterEqual(b['marker_refutation']['stats']['hits'],2)
    def test_collision_cannot_hide_changed_exact_values(self):
        _,b=self.pair(DECL+'''(rule () ((union (N 1) (N 3))) :ruleset change)
'''+FACTS+'''(run-schedule (seq (run consume) (run change) (run consume)))
(check (Seen (N 0) 0))''', {'CELL_CPU_HASH_BITS':'0'})
        self.assertGreater(b['marker_refutation']['stats']['invalidated'],0)
    def test_function_reply_changes_with_same_key(self):
        decl=DECL.replace('(R y z)', '(= cost (Cost y)) (> cost 10)')
        # Declare Cost before the rule using it; retain two real residual reads.
        decl=decl.replace('(function View', '(function Cost (E) i64 :merge (max old new))\n(function View')
        source=decl+'''(rule () ((set (Cost (N 1)) 20)) :ruleset change)
(rule () ((set (View (N 0) 0) ()) (set (View (N 0) 1) ())) :ruleset make)
'''+'''(set (Cost (N 1)) 0) (set (Cost (N 2)) 0)
'''+FACTS.replace('(set (View (N 0) 0) ()) (set (View (N 0) 1) ())','')+'''
(run-schedule (seq (run make) (run consume) (run change) (run consume)))
(check (Seen (N 0) 0))'''
        _,b=self.pair(source, {'CELL_CPU_ENTRY_PROGRAMS':'0','CELL_CPU_HASH_BITS':'0'})
        self.assertGreater(b['marker_refutation']['stats']['admitted'],0)
        self.assertGreater(b['marker_refutation']['stats']['invalidated'],0)
    def test_subsume_delete_and_recreate(self):
        _,b=self.pair(DECL+'''(rule () ((subsume (P (N 0) (N 1))) (delete (R (N 3) (N 4)))) :ruleset hide)
(rule () ((R (N 2) (N 4)) (set (View (N 0) 2) ())) :ruleset change)
'''+FACTS+'''(run-schedule (seq (run consume) (run hide) (run consume) (run change) (run consume)))
(check (Seen (N 0) 2))''')
        self.assertGreater(b['marker_refutation']['stats']['invalidated'],0)
    def test_delta_partition_is_not_absence(self):
        decl=DECL.replace('((View x t) (P x y) (R y z))', '((Trigger x t) (View x 0) (P x y) (R y z))')
        source=decl+'''(rule () ((Trigger (N 0) 0) (P (N 0) (N 1))
  (R (N 1) (N 2)) (Trigger (N 0) 1)) :ruleset make)
(set (View (N 0) 0) ())
(run-schedule (seq (run make) (run consume)))
(check (Seen (N 0) 0) (Seen (N 0) 1))'''
        _,b=self.pair(source, {'CELL_CPU_ENTRY_PROGRAMS':'0','CELL_CPU_LOCAL_PROJECTIONS':'0'})
        self.assertGreater(b['marker_refutation']['stats']['delta_incomplete'],0)
    def test_checkpoint_does_not_restore_expired_receipts(self):
        source=DECL+'''(rule () ((R (N 1) (N 4))) :ruleset change)
'''+FACTS+'''(run-schedule (seq (run consume) (run change) (run consume)))
(check (Seen (N 0) 0))'''
        self.pair(source)
        src=self.root/'source.egg';p=build(prepare(src,FRONTEND),reference_tables=['View'])
        env={'CELL_CPU_MARKER_REFUTATION':'1','CELL_CPU_SIEVE':'0','CELL_CPU_REFERENCE_VIEWS':'1','CELL_CPU_COMPOSITIONS':'0'}
        execute(p,self.root/'partial',ENGINE,checkpoint_after=1,environment=env)
        execute(p,self.root/'resume',ENGINE,resume=self.root/'partial/runtime/checkpoint.json',environment=env)
        self.assertEqual(json.loads((self.root/'resume/runtime/state.json').read_text()),json.loads((self.root/'1/runtime/state.json').read_text()))

    def test_multistage_recipe_shares_negative_without_marker_rows(self):
        _,b=self.pair(MULTISTAGE, {'CELL_CPU_REFUTATION_SCOPE':'multistage'})
        stats=b['marker_refutation']['stats']
        self.assertGreater(stats['continuation_entries'],0)
        self.assertGreater(stats['later_epoch_hits'],0)
        self.assertGreater(stats['other_ruleset_hits'],0)
        self.assertGreaterEqual(stats['max_continuation_depth'],1)
        self.assertFalse(any(r['table']=='View' for r in json.loads((self.root/'1/runtime/state.json').read_text())['rows']))

    def test_multistage_late_input_reopens_the_future_query(self):
        source=MULTISTAGE.replace('(run growB)', '(run change) (run growB)')+'\n(check (Seen (N 0) 2))'
        _,b=self.pair(source, {'CELL_CPU_REFUTATION_SCOPE':'multistage'})
        self.assertGreater(b['marker_refutation']['stats']['invalidated'],0)

MULTISTAGE='''(datatype E (N i64) (Seed E) (A E) (B E) (P E E) (R E E) (Seen E i64))
(function View (E i64) Unit :merge old)
(ruleset growA) (ruleset readA) (ruleset growB) (ruleset readB) (ruleset change)
(rule ((Seed x)) ((A x)) :ruleset growA)
(rule ((A x) (Seed x) (P x y) (R y z)) ((Seen x 1)) :ruleset readA)
(rule ((A x)) ((B x)) :ruleset growB)
(rule ((B x) (A x) (P x y) (R y z)) ((Seen x 2)) :ruleset readB)
(rule () ((R (N 1) (N 4))) :ruleset change)
(Seed (N 0)) (P (N 0) (N 1)) (P (N 0) (N 2))
(R (N 3) (N 4)) (R (N 4) (N 3))
(run-schedule (seq (run growA) (run readA) (run growB) (run readB)))'''

if __name__=='__main__': unittest.main()

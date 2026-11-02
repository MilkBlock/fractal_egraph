"""Causal survey checks; repeated projections are never executable certificates."""
import copy
import json
import tempfile
import unittest
from pathlib import Path

from eggfuse.engine.cell_runtime import execute, prepare
from eggfuse.ir.cell_runtime import build
from experiments.cell_runtime.reference_views_test import ENGINE, FRONTEND
from experiments.cell_runtime.union_bake_survey import bake_requests, recurrence, signature, summarize


def touch(event=0, slot=0):
    return dict(event=event, slot=slot, leaf=True, kind='input_external',
                peer_slots=[], peer_outputs=[], opponents={'N': 1})


def observation(epoch=1, frame=1):
    return dict(epoch=epoch, frame=frame, template=0, kind='original_rule',
                completed=True, born_this_run=False, active_ruleset='alias',
                continuation_depth=1, owned_fact_repair_visits=1,
                before_input_partition=[[0, 0]], after_input_partition=[[0, 0]],
                before_output_partition=[], after_output_partition=[],
                new_input_output_aliases=[], input_operator_refinements=[],
                input_operator_before={'0': {'N': 1}}, scalar_values=[],
                touches=[touch()], bake_input_touches=[touch()], stage_path=[0])


class CausalProjectionTests(unittest.TestCase):
    def test_no_within_epoch_target_training(self):
        rows=[observation(1, 1), observation(1, 2), observation(2, 3)]
        r=recurrence(rows)
        self.assertEqual(r['unseen_before_epoch'], 2)
        self.assertEqual(r['same_observed_target'], 1)
        self.assertEqual(r['different_frame_same_target'], 1)

    def test_derived_touches_are_not_bake_inputs(self):
        a=observation(); b=copy.deepcopy(a)
        b['touches'].append(touch(3, 9))
        self.assertEqual(signature(a), signature(b))

    def test_event_order_and_same_event_correlation_preserved(self):
        a=observation(); a['bake_input_touches']=[touch(3, 1), touch(4, 2)]
        b=copy.deepcopy(a); b['bake_input_touches']=[touch(30, 1), touch(40, 2)]
        self.assertEqual(signature(a), signature(b))
        b['bake_input_touches']=[touch(40, 1), touch(30, 2)]
        self.assertNotEqual(signature(a), signature(b))
        b['bake_input_touches']=[touch(3, 1), touch(3, 2)]
        self.assertNotEqual(signature(a), signature(b))

    def test_source_state_matters(self):
        a=observation(); b=copy.deepcopy(a)
        b['input_operator_before']['0']['F']=1
        self.assertNotEqual(signature(a), signature(b))
        self.assertEqual(signature(a, stateful=False), signature(b, stateful=False))

    def test_ambiguous_projection_stays_ambiguous(self):
        rows=[observation(i, i) for i in (1, 2, 3)]
        rows[1]['after_output_partition']=[[0, 0]]
        r=recurrence(rows)
        self.assertEqual(r['new_target_for_known_key'], 1)
        self.assertEqual(r['ambiguous_prior_targets'], 1)
        self.assertEqual(r['multiple_target_signatures'], 1)

    def test_initial_direct_prefix_excludes_later_apply_and_derived(self):
        rows=[observation(1, i+1) for i in range(3)]
        for i,r in enumerate(rows):r['touches']=[touch(i)]
        report={'union_impact': {'schema':2, 'scope':'test', 'rows':rows,
                 'effective_unions':3, 'scanned_frames':3, 'unready_inputs':0},
                'steps':[], 'union_lineage':{'events':[
                    {'epoch':1, 'cause':{'Apply':{'rule':2}}},
                    {'epoch':1, 'cause':{'Congruence':{}}},
                    {'epoch':1, 'cause':{'Apply':{'rule':4}}}]}}
        p={'sha256':'test', 'recipe':{'control':[], 'rules':[
            {'metadata':{'name':'test', 'source_set':'make'}, 'slots':[], 'query':[], 'effects':[]}]}}
        r=summarize(report,p)
        self.assertEqual(r['direct_apply_input_impacts'],2)
        self.assertEqual(r['initial_apply_input_impacts'],1)
        self.assertEqual(r['derived_only_input_impacts'],1)
        requests=bake_requests(r,report,p)
        self.assertFalse(requests['requests'][0]['ready_for_runtime'])
        self.assertEqual(requests['requests'][0]['cohorts'][0]['trigger_rules'],[2])
        self.assertEqual(r['certified_cell_upgrades'],0)

    def test_legacy_report_rejected(self):
        with self.assertRaisesRegex(ValueError,'rerun survey'):
            summarize({'union_impact':{'rows':[]}}, {'recipe':{'rules':[]}})


class ObserverTests(unittest.TestCase):
    def test_real_union_chain_keeps_graph_and_bindings(self):
        source='''(datatype E (N i64) (F E) (G E) (H E) (Link E E) (Witness E E))
(ruleset make) (ruleset observe) (ruleset alias)
(rule ((= x (N n))) ((G (F x))) :ruleset make)
(rule ((= f (F x))) ((H f)) :ruleset observe)
(rule ((Link a b)) ((Witness a b) (union a b)) :ruleset alias)
(N 1) (N 2) (N 3) (Link (N 1) (N 2)) (Link (N 2) (N 3))
(run-schedule (seq (run make) (run observe) (run alias)))
(check (= (G (F (N 1))) (G (F (N 3)))))'''
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp); src=root/'source.egg'; src.write_text(source)
            p=build(prepare(src,FRONTEND)); reports=[]
            for value in ('0','1'):
                reports.append(execute(p, root/value, ENGINE, audit=True, environment={
                    'CELL_CPU_UNION_IMPACT':value,'CELL_CPU_UNION_GRAFT':'0',
                    'CELL_CPU_UNION_TRACE':'0','CELL_CPU_CONGRUENCE':'0',
                    'CELL_CPU_MARKER_REFUTATION':'0','CELL_CPU_ABSORB':'0'}))
            for name in ('state.json','stages.json','binding-audit.json'):
                self.assertEqual(*[json.loads((root/v/'runtime'/name).read_text()) for v in ('0','1')],name)
            report=reports[1]; rows=report['union_impact']['rows']; events=report['union_lineage']['events']
            self.assertEqual(len(rows),len({(r['epoch'],r['frame']) for r in rows}))
            original=[r for r in rows if r['kind']=='original_rule']
            self.assertTrue(any(r['born_this_run'] for r in original))
            self.assertTrue(any(not r['born_this_run'] for r in original))
            self.assertTrue(any(len(r['events'])>1 for r in original))
            for r in rows:
                self.assertEqual(set(map(str, (s['slot'] for s in r['input_slots']))),set(r['input_operator_before']))
                for t in r['touches']:
                    self.assertIn(t['event'],r['events'])
                    self.assertEqual(events[t['event']]['epoch'],r['epoch'])
            self.assertTrue(any(any('Apply' in events[t['event']]['cause'] for t in r['touches']) and
                                any('Congruence' in events[t['event']]['cause'] for t in r['touches']) for r in original))
            result=summarize(report,p)
            self.assertGreater(result['initial_apply_input_impacts'],0)
            self.assertEqual(result['certified_cell_upgrades'],0)
            self.assertEqual(report['union_impact']['unready_inputs'],0)


if __name__=='__main__':unittest.main()

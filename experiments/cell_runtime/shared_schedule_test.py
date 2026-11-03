"""Compiled event-driven entry nodes and borrowed consumer parameter views."""
import json,subprocess,unittest
from experiments.cell_runtime import stage_spine_test as st
from experiments.cell_runtime import retained_test as rt
from eggfuse.engine.cell_runtime import prepare,execute
from eggfuse.ir.cell_runtime import build
from eggfuse.ir.recipe import digest

BASE={'CELL_CPU_STAGE_SPINE':'1','CELL_CPU_STAGE_SPINE_PARTIAL':'1','CELL_CPU_STAGE_SPINE_SHARED_ENTRIES':'1','CELL_CPU_STAGE_SPINE_RECEIPTS':'1'}
class SharedScheduleTests(st.StageSpineTests):
    MODES=({**BASE,'CELL_CPU_SHARED_NODE_SCHEDULE':'0'},{**BASE,'CELL_CPU_SHARED_NODE_SCHEDULE':'1'})
    def test_shared_parameters_are_not_reassembled_for_each_consumer(self):
        _,(before,after)=self.pair('''(datatype E (N i64) (Seed E) (P E) (Q E) (Out E i64))
(ruleset a) (ruleset b)
(rule ((Seed x)) ((P x) (Q x)) :ruleset a)
(rule ((P x) (Q x)) ((Out x 1)) :ruleset b)
(rule ((P renamed) (Q renamed)) ((Out renamed 2)) :ruleset b)
(rule ((P other) (Q other)) ((Out other 3)) :ruleset b)
(Seed (N 0)) (run-schedule (seq (run a) (run b)))''')
        s=after['entry_schedule']['stats']
        self.assertGreater(sum(before['stage_spine']['stats']['entry_requests_by_rule'].values()),0)
        self.assertEqual(sum(after['stage_spine']['stats']['entry_requests_by_rule'].values()),0)
        self.assertLess(s['parameters_copied'],s['borrowed_slots'])
        self.assertGreater(s['consumer_seed_deliveries'],s['predicate_checks'])
    def test_slow_consumer_has_its_own_watermark(self):
        self.pair('''(datatype E (N i64) (Seed E) (P E) (Out E i64))
(ruleset produce) (ruleset left) (ruleset right) (ruleset late)
(rule ((Seed x)) ((P x)) :ruleset produce)
(rule ((P x)) ((Out x 1)) :ruleset left)
(rule ((P renamed)) ((Out renamed 2)) :ruleset right)
(rule () ((Seed (N 1))) :ruleset late)
(Seed (N 0)) (run-schedule (seq (run produce) (run left) (run late) (run produce) (run left) (run right)))''')
    def test_literal_alias_filters_refresh_after_union(self):
        self.pair('''(datatype E (N i64) (Edge E E) (Out E i64))
(let fixed (N 1)) (let other (N 0)) (ruleset consume) (ruleset change)
(rule ((Edge fixed fixed)) ((Out fixed 1)) :ruleset consume)
(rule ((Edge x x)) ((Out x 2)) :ruleset consume)
(Edge other fixed)
(rule () ((union fixed other)) :ruleset change)
(run-schedule (seq (run consume) (run change) (run consume)))''')
    def test_function_key_coalescence_retires_old_packets(self):
        self.pair('''(datatype E (N i64) (Out E i64)) (function Cost (E) i64 :merge (+ old new))
(ruleset consume) (ruleset change)
(rule ((= n (Cost x))) ((Out x n)) :ruleset consume)
(set (Cost (N 0)) 2) (set (Cost (N 1)) 3)
(rule () ((union (N 0) (N 1))) :ruleset change)
(run-schedule (seq (run consume) (run change) (run consume)))''')
    def test_scheduler_is_independent_of_stage_spine(self):
        self.MODES=({'CELL_CPU_STAGE_SPINE':'0','CELL_CPU_SHARED_NODE_SCHEDULE':'0'},
                    {'CELL_CPU_STAGE_SPINE':'0','CELL_CPU_SHARED_NODE_SCHEDULE':'1'})
        _,(_,r)=self.pair('''(datatype E (N i64) (Out E i64)) (function Cost (E) i64 :merge (+ old new))
(ruleset consume) (ruleset change)
(rule ((= n (Cost x))) ((Out x n)) :ruleset consume)
(set (Cost (N 0)) 2) (set (Cost (N 1)) 3)
(rule () ((union (N 0) (N 1))) :ruleset change)
(run-schedule (seq (run consume) (run change) (run consume)))''')
        self.assertTrue(r['entry_schedule']['enabled'])
    def test_index_reset_does_not_erase_node_notifications(self):
        self.MODES=tuple({**m,'CELL_CPU_QUERY_INDEX_SCOPE':'table'} for m in self.MODES)
        self.pair(st.CHAIN+'(run-schedule (seq (run a) (run b) (run c)))')
    def test_legacy_artifact_needs_reemission_only_for_new_mode(self):
        src=self.root/'source.egg';src.write_text(st.CHAIN+'(run-schedule (run a))');p=build(prepare(src,rt.FRONTEND))
        del p['entry_schedule'];p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
        execute(p,self.root/'legacy',rt.ENGINE,environment=self.MODES[0])
        with self.assertRaises(subprocess.CalledProcessError):execute(p,self.root/'unsupported',rt.ENGINE,environment=self.MODES[1])
        self.assertIn('requires compiler entry_schedule',(self.root/'unsupported/runtime.log').read_text())
    def test_coalesced_changes_preserve_epoch_even_when_value_returns(self):
        _,(_,r)=self.pair('''(function Input () i64 :merge new)
(function Count () i64 :merge (+ old new)) (ruleset consume) (ruleset up) (ruleset down)
(rule ((= value (Input))) ((set (Count) 1)) :ruleset consume)
(rule () ((set (Input) 1)) :ruleset up)
(rule () ((set (Input) 0)) :ruleset down)
(set (Input) 0) (set (Count) 0)
(run-schedule (seq (run consume) (run up) (run down) (run consume))) (check (= (Count) 2))''')
        self.assertGreater(r['entry_schedule']['stats']['parameter_packets_reused'],0)
    def test_forged_compiled_projection_is_rejected(self):
        src=self.root/'source.egg';src.write_text(st.CHAIN+'(run-schedule (run a))');p=build(prepare(src,rt.FRONTEND))
        p['entry_schedule']['nodes'][0]['consumers'][0]['slots'][0]['port']=999
        p['sha256']=digest({k:v for k,v in p.items() if k!='sha256'})
        path=self.root/'bad.json';path.write_text(json.dumps(p))
        proc=subprocess.run([str(rt.ENGINE),'--program',str(path),'--output',str(self.root/'bad')],capture_output=True,text=True)
        self.assertNotEqual(proc.returncode,0);self.assertIn('shared entry schedule differs',proc.stderr)

"""Source-table continuations: source equivalence and contract rejection.

Run with the existing cell-spine solver Python. No Analyze/benchmark is rerun.
"""
import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from tools.cell_spine.pass3_markers import compile_source
from eggfuse.compiler.marker_ir import build, validate, digest
from eggfuse.compiler.pipeline import native_core
from eggfuse.compiler.verify import compare_source

ROOT=Path(__file__).resolve().parents[2]
TOOLS=ROOT/'target/debug/examples'

BASE='''(datatype E (N i64) (A E) (B E) (C E) (D E) (Out E i64))
(relation Seed (E)) (relation Gate (E))
(function Cost (E) i64 :merge (min old new))
(ruleset build) (ruleset consume) (ruleset late) (ruleset improve) (ruleset alias) (ruleset hide)
(rule ((Seed x)) ((B (A x))) :ruleset build :name "producer")
(rule ((= r (B (A (N n)))) (= c (Cost (N n))) (> c 2) (Gate (N n)))
      ((C r) (Out r c)) :ruleset consume :name "left")
(rule ((= r (B (A (N n)))) (= c (Cost (N n))) (> c 2) (Gate (N n)))
      ((D r)) :ruleset consume :name "right")
(rule ((Seed x)) ((set (Cost x) 9) (Gate x)) :ruleset late :name "late-port")
(rule ((Seed x)) ((set (Cost x) 5)) :ruleset improve :name "updated-port")
(rule ((Seed x)) ((union (A x) (A (N 2)))) :ruleset alias :name "new-connection")
'''

CASES={
 'consumer-bridge':'''(datatype E (N i64) (Branch E E) (Read E i64) (Left E E E) (Right E E E)) (ruleset s)
(rule ((= root (Branch x y)) (= u (Read x i)) (= v (Read y i))) ((Left root u v)) :ruleset s :name "left")
(rule ((= root (Branch x y)) (= u (Read x i)) (= v (Read y i))) ((Right root u v)) :ruleset s :name "right")
(Branch (N 0) (N 1)) (Read (N 0) 0) (Read (N 1) 0) (Read (N 9) 0)
(run-schedule (saturate s))''',
 'bool-literal':'''(datatype E (N i64) (Tag bool E) (Wrap E) (C E) (D E)) (ruleset s)
(rule ((= r (Wrap (Tag true (N n))))) ((C r)) :ruleset s :name "one")
(rule ((= r (Wrap (Tag true (N n))))) ((D r)) :ruleset s :name "two")
(Wrap (Tag true (N 1))) (Wrap (Tag false (N 2))) (run s 1)''',
 'late-update':BASE+'''(Seed (N 1))
(run build 1) (run consume 1) (run late 1) (run consume 1)
(run improve 1) (run consume 1) (run consume 1)
''',
 'union':BASE+'''(Seed (N 1)) (Seed (N 2))
(run build 1) (run late 1) (run consume 1)
(run alias 1) (run consume 1)
''',
 'subsumed':BASE+'''(rule ((Seed x)) ((subsume (B (A x)))) :ruleset hide :name "hide")
(Seed (N 1)) (run build 1) (run consume 1) (run hide 1) (run late 1) (run consume 1)
''',
 'finite-repeat':BASE+'''(Seed (N 1))
(run-schedule (repeat 2 (seq (run consume) (run late) (run build))))
''',
 'effectful':BASE.replace('((C r) (Out r c))','((set (Cost (N n)) (- c 1)) (C r) (Out r c))')+'''
(Seed (N 1)) (run build 1) (run late 1) (run-schedule (repeat 3 (run consume)))
''',
}


def compile_case(source,folder):
    folder.mkdir(parents=True);src=folder/'source.egg';src.write_text(source)
    report=compile_source(src,folder/'empty-analysis',folder/'compiled',TOOLS/'lhs_ast')
    marked=folder/'compiled/published.egg'
    core=native_core(TOOLS/'eggfuse_frontend',marked,folder)
    ir=build(report,core,marked.read_bytes(),b'')
    (folder/'marker-program.json').write_text(json.dumps(ir,indent=2))
    (folder/'core.json').write_text(json.dumps(core))
    return report,core,ir


class SpineStates(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(prefix='spine-state-',dir=ROOT/'out')
        cls.root=Path(cls.tmp.name);cls.cases={}
        for name,source in CASES.items():
            folder=cls.root/name;report,core,ir=compile_case(source,folder)
            for tag,path in [('source',folder/'source.egg'),('marked',folder/'compiled/published.egg')]:
                p=subprocess.run([str(TOOLS/'eggfuse_frontend'),str(path),'--native-full','--export-to',str(folder/tag)],text=True,capture_output=True)
                if p.returncode:raise AssertionError(p.stderr)
            a=json.loads((folder/'source/native-state.json').read_text());b=json.loads((folder/'marked/native-state.json').read_text())
            compare_source(a,b,ir)
            cls.cases[name]=(report,core,ir,b)

    @classmethod
    def tearDownClass(cls):cls.tmp.cleanup()

    def test_exact_source_admission_without_bake_coverage(self):
        r,_,m,_=self.cases['late-update']
        self.assertGreater(r['selection_diagnostics']['source_only_fragments'],0)
        self.assertTrue(m['markers']);self.assertTrue(m['continuations'])
        self.assertTrue(any(v['parent'] for v in r['markers']))
        self.assertGreater(r['variants']['published']['publish_actions'],0)

    def test_scalar_join_is_anchored_to_its_consumer_structure(self):
        r,_,_,state=self.cases['consumer-bridge']
        self.assertTrue(any(len(v['body'])==3 for v in r['markers']))
        for v in r['markers']:
            self.assertIn('Branch',[f['args'][1]['head'] for f in v['body']])
        self.assertEqual(sum(row['table']=='Left' for row in state['rows']),1)

    def test_late_and_changed_ports_remain_live(self):
        _,_,m,state=self.cases['late-update']
        self.assertTrue(any(a['call'].get('table')=='Cost' for s in m['continuations'] for a in s['ports']))
        self.assertTrue(any(r['table'].startswith('CellSpineApplied') for r in state['rows']))
        costs={r['values'][1]['value'] for r in state['rows'] if r['table']=='Out'}
        self.assertEqual(costs,{5,9})

    def test_effectful_receipt_is_not_a_permanent_done_bit(self):
        _,_,m,_=self.cases['effectful']
        left=next(s for s in m['continuations'] if s['identity'][1]=='left')
        self.assertEqual(left['completion'],'historical-observation-only')

    def test_single_consumer_can_be_selected_by_lifecycle_cost(self):
        from tools.cell_spine.compiler import digest as file_digest
        folder=self.root/'single-consumer';folder.mkdir()
        source=folder/'source.egg'
        source.write_text('''(datatype E (N i64) (A E) (B E) (C E)) (ruleset s)
(rule ((= r (B (A (N n))))) ((C r)) :ruleset s :name "one")
(B (A (N 1))) (run s 1)''')
        r=compile_source(source,folder/'empty',folder/'baseline',TOOLS/'lhs_ast')
        self.assertTrue(r['offered_fragments'])
        prices=dict(schema='marker-cost-profile/v1',source_sha256=file_digest(source),provenance='synthetic test only',markers={g['key']:dict(maintenance_ns=1,saved_search_ns_by_rule={n:100*g['atoms'] for n in g['consumers']}) for g in r['offered_fragments']})
        r=compile_source(source,folder/'empty',folder/'selected',TOOLS/'lhs_ast',profile=prices)
        self.assertTrue(r['markers']);self.assertEqual(r['modified_consumers'],1)

    def test_subsumed_structure_is_not_permanently_certified(self):
        _,_,m,state=self.cases['subsumed']
        self.assertFalse(any(f['args'][1]['head']=='B' for v in m['markers'] for f in v['guarantee']))
        self.assertFalse(any(r['table'] in ('C','D','Out') for r in state['rows']))

    def test_contract_rejects_forged_completion_and_lost_keys(self):
        _,core,ir,_=self.cases['effectful']
        for mutation in ('completion','key','port','guarantee'):
            bad=copy.deepcopy(ir)
            s=next(s for s in bad['continuations'] if s['identity'][1]=='left')
            if mutation=='completion':s['completion']='immutable-constructor-effects'
            elif mutation=='key':s['keys'].pop()
            elif mutation=='port':s['ports'].clear()
            else:bad['markers'][0]['guarantee'].pop()
            bad['sha256']=digest({k:v for k,v in bad.items() if k!='sha256'})
            with self.assertRaises((ValueError,KeyError)):validate(bad,core)


if __name__=='__main__':unittest.main()

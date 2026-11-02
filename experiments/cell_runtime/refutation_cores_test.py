"""Proof projection, numeric implication, invalidation and exact stage behavior."""
import json,tempfile,unittest,subprocess
from pathlib import Path
from eggfuse.engine.cell_runtime import prepare,execute
from eggfuse.ir.cell_runtime import build
from eggfuse.engine.cpu import compare_native
from experiments.cell_runtime.reference_views_test import FRONTEND,ENGINE
from experiments.cell_runtime.presearch_bench import SETTINGS
from experiments.cell_runtime.refutation_test import DECL,FACTS

PORT='''(datatype T (Term i64)) (datatype C (Pair T i64) (Choose C C))
(function F (i64) C :merge (Choose old new))
(function View (C i64) Unit :merge old) (relation Seen (i64 T))
(ruleset publish) (ruleset consume) (ruleset change) (ruleset hide)
(rule ((Choose (Pair t limit) right)) ((set (View right limit) ())) :ruleset publish)
(rule ((View port limit) (= port (Pair t cost)) (> cost limit)) ((Seen limit t)) :ruleset consume)
(Choose (Pair (Term 0) 5) (Pair (Term 1) 4))
(Choose (Pair (Term 0) 8) (Pair (Term 1) 4))
(Choose (Pair (Term 0) 12) (Pair (Term 1) 4))
'''

class CoreTests(unittest.TestCase):
    def setUp(self):self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
    def tearDown(self):self.temp.cleanup()
    def pair(self,source,extra=None):
        src=self.root/'source.egg';src.write_text(source);core=prepare(src,FRONTEND)
        p=build(core,reference_tables=['View']);reports={}
        for variant in ('off','legacy','scoped','cores','bounds','shadow'):
            env={**SETTINGS,'CELL_CPU_REFUTATION_SCOPE':'multistage','CELL_CPU_REFUTATION_VARIANT':'legacy' if variant=='off' else variant,
                'CELL_CPU_MARKER_REFUTATION':'0' if variant=='off' else '1','CELL_CPU_REFERENCE_VIEWS':'1',**(extra or {})}
            reports[variant]=execute(p,self.root/variant,ENGINE,audit=True,environment=env)
        for file in ('stages.json','binding-audit.json','state.json'):
            base=json.loads((self.root/'off/runtime'/file).read_text())
            for variant in reports:self.assertEqual(base,json.loads((self.root/variant/'runtime'/file).read_text()),(variant,file))
        subprocess.run([str(FRONTEND),str(src),'--native-full','--export-to',str(self.root/'native')],check=True,capture_output=True)
        compare_native(core,json.loads((self.root/'bounds/runtime/state.json').read_text()),json.loads((self.root/'native/native-state.json').read_text()))
        return p,reports
    def test_failure_slice_discards_unread_binding_fields(self):
        decl=DECL.replace('(function View','(relation Tail (E i64))\n(function View').replace('(R y z))','(R y z) (Tail z t))')
        facts=''.join(f'(Tail (N {99+i}) {tag})\n' for tag in (0,1) for i in range(20))
        _,r=self.pair(decl+FACTS+facts+'(run-schedule (run consume))',{'CELL_CPU_ENTRY_PROGRAMS':'0'})
        self.assertGreater(r['cores']['marker_refutation']['stats']['admitted_sliced_proofs'],0)
        self.assertGreater(r['cores']['marker_refutation']['stats']['stronger_query_hits'],0)
    def test_complete_port_bound_handles_stronger_guard(self):
        _,r=self.pair(PORT+'(run-schedule (seq (run publish) (run consume)))')
        self.assertGreater(r['bounds']['marker_refutation']['stats']['bound_hits'],0)
        self.assertGreater(r['bounds']['marker_refutation']['stats']['bound_changed_threshold_hits'],0)
        self.assertGreater(r['bounds']['marker_refutation']['stats']['compiled_hits'],0)
    def test_union_adds_winning_evidence_and_expires_bound(self):
        _,r=self.pair(PORT+'''(rule () ((union (Pair (Term 1) 4) (Pair (Term 2) 20))) :ruleset change)
(run-schedule (seq (run publish) (run consume) (run change) (run consume)))
(check (Seen 5 (Term 2)) (Seen 8 (Term 2)) (Seen 12 (Term 2)))''',{'CELL_CPU_HASH_BITS':'0'})
        self.assertGreater(r['bounds']['marker_refutation']['stats']['invalidated'],0)
    def test_weaker_guard_is_not_rejected(self):
        self.pair(PORT+'''(rule () ((set (View (Pair (Term 1) 4) 2) ())) :ruleset change)
(run-schedule (seq (run publish) (run consume) (run change) (run consume)))
(check (Seen 2 (Term 1)))''')
    def test_signed_boundaries_and_guard_polarities(self):
        root=self.root
        try:
            for op,cost,limits in [('>',-9223372036854775808,[-2,9223372036854775807]),('<',9223372036854775807,[0,-9223372036854775808]),('>=',4,[5,8]),('<=',4,[3,1])]:
                self.root=root/(op.replace('>','gt').replace('<','lt').replace('=','eq'));self.root.mkdir()
                source=PORT.replace('(> cost limit)',f'({op} cost limit)').replace('(Term 1) 4',f'(Term 1) {cost}')
                source=source.replace('(Term 0) 5',f'(Term 0) {limits[0]}').replace('(Term 0) 8',f'(Term 0) {limits[1]}').replace('(Term 0) 12',f'(Term 0) {limits[1]}')
                self.pair(source+'(run-schedule (seq (run publish) (run consume)))')
        finally:self.root=root
    def test_cold_resume_and_subsumed_port(self):
        p,_=self.pair(PORT+'''(rule () ((subsume (Pair (Term 1) 4)) (set (View (Pair (Term 1) 4) 16) ())) :ruleset hide)
(run-schedule (seq (run publish) (run consume) (run hide) (run consume)))''')
        env={**SETTINGS,'CELL_CPU_MARKER_REFUTATION':'1','CELL_CPU_REFUTATION_VARIANT':'bounds','CELL_CPU_REFUTATION_SCOPE':'multistage','CELL_CPU_REFERENCE_VIEWS':'1'}
        execute(p,self.root/'pause',ENGINE,checkpoint_after=2,environment=env)
        execute(p,self.root/'resume',ENGINE,resume=self.root/'pause/runtime/checkpoint.json',environment=env)
        self.assertEqual(json.loads((self.root/'resume/runtime/state.json').read_text()),json.loads((self.root/'bounds/runtime/state.json').read_text()))

if __name__=='__main__':unittest.main()

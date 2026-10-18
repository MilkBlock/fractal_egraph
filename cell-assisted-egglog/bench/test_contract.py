import copy
import json
from pathlib import Path
import tempfile
import unittest
import run

class Contract(unittest.TestCase):
    def timed(self):
        return dict(graph_snapshot_requested=False,setup_seconds=.1,input_seconds=.1,run_seconds=.8,core_seconds=1.,progress=[False],
            stats=dict(verification=dict(enabled=False,batches=0),audit_uncovered=[],phase_seconds=dict(native_events_seconds=.1,device_seconds=.01)))
    def test_timing_rejects_oracles_exports_and_overlap(self):
        run.assert_timed(self.timed(),True)
        for edit in [lambda r:r.update(graph_snapshot_requested=True),lambda r:r['stats']['verification'].update(enabled=True),
                     lambda r:r.update(core_seconds=3),lambda r:r['stats']['phase_seconds'].update(install_seconds=1),lambda r:r.update(progress=[True])]:
            r=self.timed();edit(r)
            with self.assertRaises(AssertionError):run.assert_timed(r,True)
    def test_graph_gate_checks_values_equivalence_and_exact_row_visibility(self):
        rows=[dict(op='Leaf',sort='Expr',args=[['i64',str(i)]],result=str(i),subsumed=bool(i)) for i in (0,1)]
        rows += [dict(op='Best',sort='i64',args=[['Expr','0']],result='2',subsumed=False)]
        with tempfile.TemporaryDirectory() as d:
            d=Path(d);a=d/'a.json';b=d/'b.json';a.write_text(json.dumps(rows));b.write_text(json.dumps(rows))
            self.assertTrue(run.check_graph(a,b,d/'equal')['exact_equal'])
            for change in ['value','visibility','equivalence']:
                bad=copy.deepcopy(rows)
                if change=='value':bad[-1]['result']='3'
                elif change=='visibility':bad[0]['subsumed'],bad[1]['subsumed']=True,False
                else:bad[1]['result']='0'
                b.write_text(json.dumps(bad))
                with self.assertRaises(AssertionError):run.check_graph(a,b,d/change)
    def test_sparse_fixture_contains_real_closing_constraint(self):
        source,facts,success=run.workload(64,'sparse-cycle')
        self.assertIn('(L3 b dst)',source);self.assertEqual(success,3)
        self.assertEqual(facts.count('(Offer '),192)

if __name__=='__main__':unittest.main()

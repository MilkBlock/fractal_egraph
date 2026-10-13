import tempfile
import unittest
from pathlib import Path
from cell_egraph import folder_in_out, match_terms, directories, overview, state_detail

class CellTests(unittest.TestCase):
    def test_reuse_and_window_evidence_is_exposed_without_inference(self):
        import json
        with tempfile.TemporaryDirectory() as t:
            root=Path(t).resolve();run=root/'out'/'run';run.mkdir(parents=True)
            window={'unit':0,'container':1,'consumer':3,'kind':'apply_entry','members':[3]}
            reuse={'schema':'cell-reuse-value/v1','cells':[{'eclass':0,'units':[0],'interface_instances':2}]}
            registry={'nodes':[{'unit':0,'parents':[],'contract':0}], 'units':[{'unit':0,'eclass':0}], 'windows':[window], 'reuse_value':reuse}
            (run/'analysis.json').write_text(json.dumps({'saturated_rule_composition':{'cell_egraph':registry}}))
            data=overview(root,'out/run')
            self.assertEqual(data['reuse_value'],reuse)
            self.assertEqual(data['nodes'][0]['history_windows'],[window])
            detail=state_detail(root,'out/run',0)
            self.assertEqual(detail['reuse_value']['interface_instances'],2)
            self.assertFalse(detail['guarantee']['certified_body'])
    def test_precise_ground_arguments_and_hidden_rows(self):
        s={'values':[{'literal':'0'},{'literal':'1'},{'sort':'E'},{'sort':'E'},{'sort':'E'}],
           'rows':[{'op':'Num','args':[0],'result':2},{'op':'Num','args':[1],'result':3},
                   {'op':'Add','args':[2,3],'result':4},{'op':'Add','args':[3,2],'result':4}],
           'subsumed_rows':[3]}
        self.assertEqual(match_terms(s,['(Add (Num 0) (Num 1))'])[0],[2])
        self.assertEqual(match_terms(s,['(Add (Num 1) (Num 0))'])[0],[])
        self.assertEqual(match_terms(s,['(Add (Num 0) (Num 0))'])[0],[])
    def test_directories_match_source_and_sort_latest(self):
        import os
        with tempfile.TemporaryDirectory() as t:
            root=Path(t)
            for name,source,stamp in [('a','(V)\n',1),('b','(V)\r\n',3),('c','(Other)',5)]:
                p=root/'out'/name;p.mkdir(parents=True)
                (p/'source.egg').write_text(source)
                (p/'analysis.json').write_text('{}')
                os.utime(p/'analysis.json',(stamp,stamp))
            data=directories(root,'(V)')['directories']
            self.assertEqual([r['path'] for r in data if r['matches_source']],['out/b','out/a'])
            self.assertEqual(data[0]['path'],'out/c')

    def test_round_does_not_read_final_registry(self):
        import json
        with tempfile.TemporaryDirectory() as t:
            root=Path(t).resolve();run=root/'out'/'run';(run/'rounds').mkdir(parents=True)
            early={'cell_egraph':{'units':[{'unit':0,'eclass':0}],'nodes':[{'unit':0,'parents':[],'contract':0}]},'jobs':[{'candidate_id':0,'state':'Pending'}]}
            final={'cell_egraph':{'units':[{'unit':1,'eclass':1}],'nodes':[{'unit':1,'parents':[],'contract':1}]},'jobs':[]}
            (run/'rounds/manifest.json').write_text(json.dumps([{'stem':'round-0001','label':'round 1'}]))
            (run/'rounds/round-0001.json').write_text(json.dumps({'saturated_rule_composition':early}))
            (run/'analysis.json').write_text(json.dumps({'saturated_rule_composition':final}))
            self.assertEqual(overview(root,'out/run','round-0001')['nodes'][0]['unit'],0)
            self.assertEqual(overview(root,'out/run')['nodes'][0]['unit'],1)
            self.assertIsNone(state_detail(root,'out/run',0,'round-0001')['state'])
            with self.assertRaises(ValueError):overview(root,'out/run','../../analysis')
            (run/'analysis.json').unlink()
            self.assertEqual(overview(root,'out/run')['nodes'][0]['unit'],0)

    def test_path_scope(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t)
            self.assertEqual(folder_in_out(root,'out/test'),(root/'out/test').resolve())
            with self.assertRaises(ValueError):folder_in_out(root,'../outside')

if __name__=='__main__': unittest.main()

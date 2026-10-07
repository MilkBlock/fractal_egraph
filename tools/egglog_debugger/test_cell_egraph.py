import tempfile
import unittest
from pathlib import Path
from cell_egraph import folder_in_out, match_terms, directories

class CellTests(unittest.TestCase):
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

    def test_path_scope(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t)
            self.assertEqual(folder_in_out(root,'out/test'),(root/'out/test').resolve())
            with self.assertRaises(ValueError):folder_in_out(root,'../outside')

if __name__=='__main__': unittest.main()

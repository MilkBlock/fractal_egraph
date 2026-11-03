import json,subprocess,tempfile,unittest
from pathlib import Path
from eggfuse.ir.recipe import build
from eggfuse.analyzer.regions import from_core
from eggfuse.compiler.targets.metal.cell_waves import eligible,lower

ROOT=Path(__file__).resolve().parents[2]

class CellWaves(unittest.TestCase):
    def program(self,merge):
        source=f'''(datatype E (N i64) (P E) (Q E))
(function F (E) i64 :merge {merge}) (relation Seed (E i64)) (ruleset s)
(rule ((Seed x n)) ((let p (P x)) (set (F p) n)) :ruleset s)
(rule ((= p (P x)) (= n (F p))) ((Q p)) :ruleset s)
(Seed (N 0) 4) (run-schedule (saturate s))'''
        with tempfile.TemporaryDirectory(dir=ROOT/'out') as d:
            p=Path(d)/'source.egg';p.write_text(source)
            raw=subprocess.run([str(ROOT/'target/debug/examples/eggfuse_frontend'),str(p)],input='',text=True,capture_output=True,check=True)
        c=json.loads(raw.stdout.splitlines()[0]);return c,build(c),from_core(c)['batch_program']['templates']

    def test_minmax_admission_and_immutable_coordinates(self):
        for merge in ['(min old new)','(max new old)']:
            core,recipe,templates=self.program(merge)
            self.assertTrue(any(eligible(t,recipe) and len(t['steps'])>1 for t in templates))
            code,info=lower(templates,recipe,{t['name']:i for i,t in enumerate(core['tables'])},{})
            self.assertTrue(info['eligible_templates'])
            self.assertNotIn('write_row(',code);self.assertNotIn('row_lookup(',code)
            self.assertIn('query_lookup(',code);self.assertIn('cw_canon(',code)
            self.assertIn('out[6]',code) # continuation PC, not replay from step zero

    def test_noncommuting_and_custom_merge_stay_ordered(self):
        for merge in ['new','(+ old new)']:
            _,recipe,templates=self.program(merge)
            writes=[t for t in templates if any(e['kind']=='set' and e['call']['table']=='F' for step in t['steps'] for e in recipe['rules'][step['recipe_rule']]['effects'])]
            self.assertTrue(writes)
            self.assertTrue(all(not eligible(t,recipe) for t in writes))

if __name__=='__main__':unittest.main()

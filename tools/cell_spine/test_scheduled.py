import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from . import scheduled, solver
from .compiler import ROOT


class ScheduledSpines(unittest.TestCase):
    def fixture(self):
        columns=lambda *s:[{'Slot':i} for i in s]
        atoms=[dict(table='A',sorts=['E','E','E'],columns=columns(0,1,2)),
               dict(table='Ctx',sorts=['E','E','E'],columns=columns(1,3,4)),
               dict(table='Ty',sorts=['E','E','E'],columns=columns(0,5,6)),
               dict(table='C',sorts=['E','E','E'],columns=columns(7,3,8))]
        shape=dict(slots=[[f'v{i}','E'] for i in range(9)],atoms=atoms,guards=[],native_residuals=False)
        plan=dict(shape=shape,seed=0,steps=[[1,{'Columns':1}],[2,{'Columns':1}],[3,{'Columns':2}]],prefix_len=0,carried_slots=[])
        return dict(ruleset='s',rule='q',shape=shape,plans=[plan])

    def test_full_spine_and_optional_inputs_are_not_two_atom_views(self):
        q=self.fixture();opts=list(scheduled.options(q,{'A':2},10))
        self.assertEqual({x['context_atoms'] for x in opts},{1,2,3,4})
        self.assertGreater(len({json.dumps(x['layout']['anchor']) for x in opts}),4)
        self.assertEqual({x['layout']['payload'] for x in opts},{'anchor','outside','all'})
        self.assertTrue(any(x['complete_prefix'] for x in opts))
        for x in opts:self.assertEqual(x['plan']['shape'],q['shape'])

    def test_joint_solver_counts_shared_index_once_and_off_option(self):
        solver.select('scip');q=self.fixture();q2=dict(q,rule='q2')
        with tempfile.TemporaryDirectory() as tmp:
            out=Path(tmp)
            plan=scheduled.select([q,q2],{'A':2},out,(1,1,1),
                {('s','q'):dict(candidate_rows=10000,requests=20),('s','q2'):dict(candidate_rows=0,requests=0)})
            self.assertEqual([x['rule'] for x in plan['selected']],['q'])
            self.assertGreater(plan['choices'],3)
            self.assertTrue((out/'scheduled-selection.lp').is_file())

    def test_complete_multi_ruleset_source_is_preserved_not_archived(self):
        source=ROOT/'experiments/cell_spine/fixtures/stages/multi_rulesets.egg'
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='scheduled-contract-') as temp:
            out=Path(temp)/'out'
            result=subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',source,
                '--output',out,'--skip-build'],cwd=ROOT,capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            report=json.loads((out/'report.json').read_text())
            self.assertEqual(report['status'],'scheduled_symbolic');self.assertFalse(report['cellified'])
            self.assertEqual(source.read_bytes(),(out/'program.egg').read_bytes())
            self.assertFalse((out/'snapshot.egg').exists())
            plan=json.loads((out/'boundary-programs.json').read_text())
            self.assertEqual(plan['schema'],'cell-boundary-programs/v2')
            self.assertTrue(plan['rules'])

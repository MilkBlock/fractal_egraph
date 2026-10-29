import copy,json,tempfile,unittest
from pathlib import Path
from eggfuse.ir.recipe import build
from eggfuse.ir.test_recipe import E,I,var,call
from eggfuse.analyzer.regions import from_core
from eggfuse.compiler.targets.metal.generic_cell import lower
from eggfuse.engine.artifacts import validate_recipe_artifact


def fixture():
    tables=[dict(name='N',kind='constructor',schema=[I,E],merge=None)]+[dict(name=n,kind='constructor',schema=[E,E],merge=None) for n in ['P','Q']]
    rules=[dict(name='first',seminaive=True,body=[call('N',var('n',I),var('x'))],head=[dict(op='let',dst='p',sort=E,call={'table':'P'},args=[var('x')])]),
           dict(name='second',seminaive=True,body=[call('P',var('x'),var('p'))],head=[dict(op='let',dst='q',sort=E,call={'table':'Q'},args=[var('x')])])]
    return dict(schema='egglog-device-core/v1',tables=tables,rulesets=[dict(name='step',rules=rules)],schedule=[dict(repeat=2,body=dict(run='step'))],initial=[])

class ResidentScheduleTests(unittest.TestCase):
    def test_finite_control_and_mutated_certificate(self):
        core=fixture();region=from_core(core);recipe=build(core);p=lower(core,32,32,region_program=region)
        info=p['resident_schedules'];self.assertEqual(len(info['programs']),1)
        self.assertEqual(info['programs'][0]['control']['instructions'][0][0:2],[1,2])
        tape=p['generic_cell']['control']['instructions'];self.assertEqual(tape[0][0],6)
        self.assertEqual(tape[1][:2],[1,2]);self.assertGreater(tape[0][2],1)
        p['recipe_ir']=dict(schema=recipe['schema'],sha256=recipe['sha256'],scope=recipe['scope'])
        with tempfile.TemporaryDirectory() as d:
            d=Path(d);(d/'recipe-program.json').write_text(json.dumps(recipe));(d/'region-program.json').write_text(json.dumps(region))
            manifest=dict(files={'recipe-program.json':'','region-program.json':''})
            validate_recipe_artifact(d,manifest,p,core)
            p['resident_schedules']['programs'][0]['control']['instructions'][0][1]=100
            with self.assertRaisesRegex(ValueError,'private schedule certificate'):validate_recipe_artifact(d,manifest,p,core)

    def test_cache_ablation_keeps_analyzed_cells(self):
        core=fixture();region=from_core(core)
        a=lower(core,32,32,region_program=region);b=lower(core,32,32,region_program=region,spine_cache=False)
        self.assertEqual(a['region_program'],b['region_program'])
        self.assertEqual(a['batch_layout'],b['batch_layout'])
        self.assertEqual(a['resident_schedules'],b['resident_schedules'])

if __name__=='__main__':unittest.main()

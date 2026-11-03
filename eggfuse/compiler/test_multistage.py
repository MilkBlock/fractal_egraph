import copy,unittest
from .multistage import validate_runtime_options,validate_bake_environment,validate_compilation,execution_summary

class MultiStageTests(unittest.TestCase):
    def test_host_options_and_recipe_are_bound_to_kernel(self):
        opts=dict(stage_contexts=True,joint_polyspine=True,block_merge=True,matrix_saturate=True)
        manifest={'execution':opts};program={'cell_program':{'artifact_sha256':'recipe'}}
        validate_runtime_options(manifest,program,opts,{'sha256':'recipe'})
        for key in opts:
            with self.assertRaisesRegex(ValueError,'options differ'):
                validate_runtime_options(manifest,program,{**opts,key:False},{'sha256':'recipe'})
        with self.assertRaisesRegex(ValueError,'artifact differs'):
            validate_runtime_options(manifest,program,opts,None)
    def test_training_rules_and_merges_cannot_change(self):
        sample=dict(tables=[dict(name='Cost',merge='min')],rulesets=[dict(name='terms',rules=[{'body':['A'],'head':['B']}])])
        target=copy.deepcopy(sample);target['rulesets'].append(dict(name='init',rules=[{'body':[],'head':['Input']}]))
        validate_bake_environment(target,sample)
        for change in ['rule','merge']:
            bad=copy.deepcopy(sample)
            if change=='rule':bad['rulesets'][0]['rules'][0]['head']=['C']
            else:bad['tables'][0]['merge']='max'
            with self.assertRaises(ValueError):validate_bake_environment(target,bad)
    def test_flag_without_schedule_or_execution_is_not_activation(self):
        schedule={'saturate':{'run':'terms'}}
        program=dict(joint_polyspine=dict(enabled=True,program=dict(schedule=schedule)),stage_cell={'contract':{}},block_merge=True,
                     cell_program=dict(spine_program=dict(enabled=True),update_program={'ops':[]}),schedule=[schedule])
        validate_compilation(program)
        program['schedule']=[{'run':'terms'}]
        with self.assertRaisesRegex(ValueError,'no eligible scope'):validate_compilation(program)
        with self.assertRaises(ValueError):execution_summary(dict(complete_schedule=True,joint_polyspine={'calls':[]},stage_contexts={'patches':0}))

if __name__=='__main__':unittest.main()

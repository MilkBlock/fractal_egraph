import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from eggfuse.compiler.passes import PassManager
from eggfuse.engine.cell_runtime import load_pipeline_artifact
from experiments.cell_runtime.congruence_bench import eligible

class PipelineBenchmarkTests(unittest.TestCase):
    def pipeline(self, root, mixed=False):
        pm=PassManager(root);src=root/'source.egg';src.write_text('original')
        def step(name, inputs, outputs, options=None):
            def write(folder):
                for rel,value in outputs.items():
                    p=folder/rel;p.parent.mkdir(parents=True,exist_ok=True)
                    p.write_text(value if isinstance(value,str) else json.dumps(value))
            return pm.run(name,inputs,options or {},[],list(outputs),write,lambda f:None)
        sk=step('structural_keys',{'source':src},{'result/source_sk.egg':'keyed','result/report.json':{'selected_choices':['choose']}})
        analysis=step('analyze',{'source_sk':sk/'result/source_sk.egg'},{'result/cell-recipes.json':{}})
        marked=step('markers',{'source_sk':sk/'result/source_sk.egg','analysis':analysis/'result/cell-recipes.json'},{'result/published.egg':'marked'})
        ir=step('marker_ir',{'marked_source':marked/'result/published.egg','analysis':analysis/'result/cell-recipes.json'},
                {'core.json':{},'marker-program.json':{'markers':[{}]}})
        recipe=step('recipe_ir',{'core':ir/'core.json'},{'recipe-program.json':{}})
        bad=root/'wrong-core.json';bad.write_text('{"wrong":true}')
        device=step('device',{'source':marked/'result/published.egg','core':bad if mixed else ir/'core.json','marker':ir/'marker-program.json','recipe':recipe/'recipe-program.json'},
                    {'program.json':{},'source_sk_mk.egg':'marked','marker-program.json':{'markers':[{}]}},{'backend':'cell-cpu'})
        for f in ('program.json','source_sk_mk.egg','marker-program.json'):pm.publish(device/f,'eggfuse_related_files/'+f)

    def test_published_artifact_and_complete_chain_are_required(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);self.pipeline(root)
            # Test receipt wiring independently of the typed IR loader, which is
            # exercised by the actual artifact smoke test and runtime regressions.
            with patch('eggfuse.engine.cell_runtime.load_artifact',return_value={}):
                path,_,provenance=load_pipeline_artifact(root)
                self.assertEqual(path,root.resolve()/'eggfuse_related_files/program.json')
                self.assertEqual(provenance['selected_choices'],['choose'])
                self.assertEqual(provenance['marker_count'],1)
                path.write_text('{"different":true}')
                with self.assertRaisesRegex(ValueError,'published CPU artifact'):load_pipeline_artifact(root)

    def test_mixed_pass_inputs_are_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);self.pipeline(root,mixed=True)
            with self.assertRaisesRegex(ValueError,'pass wiring'):load_pipeline_artifact(root)

    def test_empty_passes_and_disabled_backend_cannot_get_credit(self):
        p={'selected_choices':['choose'],'marker_count':1}
        c={'validated':True,'congruence_reorder_safe':True,'congruence_templates':1}
        eligible(p,c)
        for provenance,capability in [({**p,'selected_choices':[]},c),({**p,'marker_count':0},c),
                                     (p,{**c,'congruence_reorder_safe':False}),(p,{**c,'congruence_templates':0})]:
            with self.assertRaises(ValueError):eligible(provenance,capability)

if __name__=='__main__':unittest.main()

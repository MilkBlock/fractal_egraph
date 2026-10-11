import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from .compiler import ROOT
from .stages import expand_archive


class ConcreteStages(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='stage-test-')
        cls.root=Path(cls.tmp.name);cls.outputs={}
        names=['finite_updates','visibility','containers','function_values','pair_multiset','extraction','included','non_idempotent_merge','until','global_merge']
        sources={n:ROOT/f'experiments/cell_spine/fixtures/stages/{n}.egg' for n in names}
        sources['upstream_list']=ROOT/'egglog/tests/web-demo/list.egg'
        sources['upstream_merge']=ROOT/'egglog/tests/repro-should-saturate.egg'
        for name,source in sources.items():
            out=cls.root/name
            subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',source,'--output',out,'--skip-build'],cwd=ROOT,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            cls.outputs[name]=out

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def frames(self,name):
        return json.loads((self.outputs[name]/'stage-bake/stages.json').read_text())['frames']

    def test_all_states_are_native_verified_and_not_claimed_parameterized(self):
        for out in self.outputs.values():
            d=json.loads((out/'report.json').read_text())
            self.assertEqual(d['status'],'compiled_stages')
            self.assertFalse(d['generalizes_bindings'])
            self.assertEqual(d['plan']['status'],'optimal')
            v=json.loads((out/'validation.json').read_text())
            for case in v['cases']:
                self.assertEqual(case['comparison']['status'],'equivalent')
                self.assertFalse(case['original_rules_executed_on_restore'])

    def test_lossless_ordered_recipes(self):
        for out in self.outputs.values():
            archive=json.loads((out/'stage-library.json').read_text())
            frames=json.loads((out/'stage-bake/stages.json').read_text())['frames']
            expected=[f['environment']+f['slot_declarations']+f['actions'] for f in frames]
            self.assertEqual(expand_archive(archive),expected)

    def test_finite_rounds_do_not_become_saturation(self):
        frames=self.frames('finite_updates')
        first=frames[0]
        self.assertIn('run one',first['command'])
        state=first['state'];score=next(r for r in state['rows'] if r['op']=='score')
        self.assertEqual(state['values'][score['result']]['literal'],'1')
        deleted=next(f for f in frames if f['command'].startswith('(fail'))
        self.assertFalse(any(r['op']=='score' for r in deleted['state']['rows']))

    def test_visibility_and_scope_survive(self):
        frames=self.frames('visibility')
        self.assertTrue(any(f['hidden_rows'] for f in frames))
        after_pop=next(f for f in frames if f['command'].startswith('(pop'))
        state=after_pop['state']
        self.assertFalse(any(r['op']=='V' and state['values'][r['args'][0]]['literal']=='2' for r in state['rows']))

    def test_final_state_remains_available(self):
        p=self.root/'post-check.egg'
        p.write_text((self.outputs['finite_updates']/'restore.egg').read_text()+'\n(check (= (score 0) 3))\n')
        subprocess.run([ROOT/'target/release/examples/native_math_baseline',p],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)

    def test_container_codecs_are_structural(self):
        for name,kinds in [('containers',['vec-of','set-of','map']),('pair_multiset',['pair','multiset-of']),('function_values',['unstable-fn'])]:
            labels={r['op'] for f in self.frames(name) for r in f['state']['rows']}
            for kind in kinds:
                self.assertTrue(any(f':{kind}:' in label for label in labels),(name,kind))

    def test_non_idempotent_merge_is_not_reapplied(self):
        frames=self.frames('non_idempotent_merge')
        scores=[]
        for f in frames:
            if 'run one' in f['command']:
                row=next(r for r in f['state']['rows'] if r['op']=='counter')
                scores.append(f['state']['values'][row['result']]['literal'])
        self.assertEqual(scores,['1','1','2'])

    def test_until_stops_at_original_boundary(self):
        f=self.frames('until')[0]
        row=next(r for r in f['state']['rows'] if r['op']=='score')
        self.assertEqual(f['state']['values'][row['result']]['literal'],'2')

    def test_native_witness_rejects_removed_subsume(self):
        report=json.loads((self.outputs['visibility']/'stage-bake/stages.json').read_text())
        frame=next(f for f in report['frames'] if f['hidden_rows'])
        frame['actions']=[a for a in frame['actions'] if not a.startswith('(subsume')]
        report['frames']=[frame]
        path=self.root/'bad-visibility.json';path.write_text(json.dumps(report))
        result=subprocess.run([ROOT/'target/release/examples/spine_stages','--verify',path],capture_output=True,text=True)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('differs from its native witness',result.stderr)

    def test_native_witness_rejects_wrong_function_value(self):
        report=json.loads((self.outputs['finite_updates']/'stage-bake/stages.json').read_text())
        frame=report['frames'][0]
        frame['actions']=[a.replace('(set (score 0) 1)','(set (score 0) 99)') for a in frame['actions']]
        report['frames']=[frame]
        path=self.root/'bad-value.json';path.write_text(json.dumps(report))
        result=subprocess.run([ROOT/'target/release/examples/spine_stages','--verify',path],capture_output=True,text=True)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('differs from its native witness',result.stderr)

    def test_rejects_cycles_and_bad_slices(self):
        a={'schema':'ordered-stage-recipes/v1','commands':['x'],'recipes':[{'parent':0,'prefix':0,'suffix':0,'middle':[0]}]}
        with self.assertRaises(ValueError):expand_archive(a)
        a['recipes'][0].update(parent=None,prefix=1)
        with self.assertRaises(ValueError):expand_archive(a)


if __name__=='__main__':unittest.main()

"""Cross-input evidence, typed anti-unification and persistent-library isolation."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from . import knowledge as k, merge_learning as ml
from .compiler import ROOT


def expr(name,x):return dict(sort='T',op=name,args=[dict(sort='T',port=x)])


def observation(x,y):
    left,right=expr('A',x),expr('B',y)
    return dict(parents=[dict(roots=[left],facts=[]),dict(roots=[right],facts=[])],
                target=dict(roots=[dict(sort='T',op='Join',args=[left,right])],facts=[]),external=[])


class Learning(unittest.TestCase):
    def test_aliases_and_types_survive_generalization(self):
        a,b=observation('x','y'),observation('z','w');p=ml.generalize(a,b)
        for v in (a,b):self.assertEqual(ml.instantiate(p,ml.bindings(p,v)),v)
        wrong=copy.deepcopy(a);wrong['target']['roots'][0]['args'][0]=expr('A','other')
        self.assertIsNone(ml.bindings(p,wrong))
        with self.assertRaises(ValueError):ml.generalize(expr('A','x'),dict(sort='Other',op='A',args=[]))

    def test_dictionary_learns_unknown_operator_pattern_without_names(self):
        ts={}
        for i,(x,y) in enumerate([('a','b'),('c','d'),('e','f')]):
            t=observation(x,y);ts[str(i)]=dict(template=t,bucket=[2],samples={str(i):['e']})
        result=ml.learn(ts)
        self.assertTrue(any(f['status']=='generalized-hypothesis' for f in result['families']))
        self.assertEqual({t for f in result['families'] for t in f['transitions']},set(ts))
        self.assertLess(result['objective'],sum(ml.size(t['template'])+1 for t in ts.values()))
        for f in result['families']:
            for tid in f['transitions']:
                self.assertEqual(ml.instantiate(f['template'],ml.bindings(f['template'],ts[tid]['template'])),ts[tid]['template'])
            self.assertIn('sources:',f['pattern'])

    def test_fact_changes_share_environment_but_rule_and_schedule_changes_do_not(self):
        a=dict(commands=[dict(kind='declaration',text='(datatype T (A))'),dict(kind='rule',text='R'),dict(kind='schedule',text='saturate')])
        b=copy.deepcopy(a);b['commands'].append(dict(kind='action',text='new input'))
        self.assertEqual(k.environment(a),k.environment(b))
        for i in (1,2):
            b=copy.deepcopy(a);b['commands'][i]['text']='different'
            self.assertNotEqual(k.environment(a),k.environment(b))

    def test_atomic_read_and_missing_list_do_not_create_library(self):
        with tempfile.TemporaryDirectory() as d:
            lib=Path(d)/'new/library.json';source=Path(d)/'a.egg';source.write_text('hello')
            r=k.listing(k.read(lib),lib,dict(commands=[]),source)
            self.assertFalse(r['registered']);self.assertFalse(lib.parent.exists())
            with k.locked(lib):k.save(lib,dict(schema=k.SCHEMA,environments={}))
            self.assertEqual(k.read(lib)['schema'],k.SCHEMA)
            self.assertEqual(list(lib.parent.glob('*.tmp')),[])

    def test_persisted_dictionary_reconstructs_all_transition_evidence(self):
        ts={}
        for x,y in [('a','b'),('c','d')]:
            p=observation(x,y);tid=ml.digest(p)
            ts[tid]=dict(template=p,bucket=[2],samples={x:['e']})
        learned=ml.learn(ts)
        value=dict(schema=k.SCHEMA,environments={'rules':dict(transitions=ts,families=learned['families'])})
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'library.json';k.save(path,value)
            raw=json.loads(path.read_text())
            self.assertTrue(all('encoding' in r and 'template' not in r for r in raw['environments']['rules']['transitions'].values()))
            self.assertEqual(k.read(path),value)

    def test_changed_source_cannot_publish_knowledge(self):
        with tempfile.TemporaryDirectory() as d:
            folder=Path(d);source=folder/'input.egg';source.write_text('old')
            session=k.Session(folder/'knowledge.json',dict(commands=[],actions=[]),source,folder/'work')
            source.write_text('new')
            with self.assertRaisesRegex(ValueError,'source changed'):session.commit([],[],folder)
            self.assertFalse((folder/'knowledge.json').exists())


class NativeLibrary(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='knowledge-test-')
        cls.folder=Path(cls.temp.name);cls.library=cls.folder/'knowledge.json';cls.outputs={}
        fixture=ROOT/'experiments/cell_spine/knowledge'
        inputs={label:(fixture/(name+'.egg')).read_text() for label,name in
                [('pair','independent'),('small','small'),('heldout','shared')]}
        for name,src in inputs.items():
            path=cls.folder/(name+'.egg');path.write_text(src);out=cls.folder/name
            r=subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',path,'--output',out,
                              '--knowledge-library',cls.library,'--skip-build'],cwd=ROOT,capture_output=True,text=True)
            if r.returncode:raise AssertionError(name+'\n'+r.stdout+r.stderr)
            cls.outputs[name]=json.loads((out/'report.json').read_text())
    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()

    def test_later_bake_imports_prior_cells_and_revalidates_all_contracts(self):
        p=self.outputs['small'];self.assertGreater(p['knowledge']['imported_cells'],0)
        self.assertGreater(p['knowledge']['reused_cells'],0)
        cases=json.loads((self.folder/'small/validation.json').read_text())['cases']
        self.assertEqual(len(cases),len(p['paths'])+1)
        self.assertTrue(all(c['comparison']['status']=='equivalent' for c in cases))
        self.assertTrue(all(not c['original_rules_executed'] for c in cases))

    def test_new_bindings_reuse_library_and_list_command_is_read_only(self):
        p=self.outputs['heldout'];self.assertGreater(p['knowledge']['reused_cells'],0)
        before=self.library.read_bytes()
        r=subprocess.run([sys.executable,'-m','tools.cell_spine.knowledge','list','--source',self.folder/'heldout.egg',
                          '--library',self.library],cwd=ROOT,capture_output=True,text=True,check=True)
        listed=json.loads(r.stdout);self.assertTrue(listed['registered']);self.assertTrue(listed['families'])
        self.assertEqual(listed['library_samples'],3)
        self.assertTrue(any(f['status']=='generalized-hypothesis' and f['support_samples']==2 for f in listed['families']))
        self.assertEqual(self.library.read_bytes(),before)
        source=self.folder/'heldout.egg';source.write_text(source.read_text()+'\n; edited\n')
        # Source hash, not merely file path, identifies current evidence.
        r=subprocess.run([sys.executable,'-m','tools.cell_spine.knowledge','list','--source',source,'--library',self.library],
                         cwd=ROOT,capture_output=True,text=True,check=True)
        self.assertFalse(json.loads(r.stdout)['registered'])
        self.assertEqual(self.library.read_bytes(),before)


if __name__=='__main__':unittest.main()

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE))
import stage_sampler as s


def fixture():
    core={'tables':[{'name':'Input','kind':'constructor','schema':[{'name':'i64','kind':'i64'},{'name':'Math','kind':'eq'}]}, {'name':'Add','kind':'constructor','schema':[{'name':'Math','kind':'eq'}]*3}], 'rulesets':[{'name':'comm','rules':[]}]}
    values=[{'sort':'i64','value':'9007199254740993'},{'sort':'Math','eq':'Value(1)'},{'sort':'Math','eq':'Value(2)'}]
    rows={(0,(0,)):(0,(0,1),False),(1,(1,1)):(1,(1,1,2),False)}
    snap={'rows':rows,'values':values,'aliases':{},'frame':0}
    stage={'ordinal':1,'ruleset':'comm','path':[0],'iterations':[0],'matches':[],'logical_matches':0}
    return core,snap,stage

class SampleTests(unittest.TestCase):
    def test_shared_port_and_expand(self):
        core,snap,stage=fixture();req={'root':2,'sort':'Math','depth':0,'context':False,'focus':'all','budget':5}
        a=s.build_sample(core,snap,snap,stage,req)
        self.assertEqual(a['stats']['ripen_inputs'],1)
        self.assertEqual(a['dot'].count(' -> c1 [label='),2)
        b=s.build_sample(core,snap,snap,stage,{**req,'expanded':[1]})
        self.assertEqual(b['stats']['visible_facts'],2)
        self.assertEqual(b['stats']['ripen_inputs'],0)
        self.assertIn('9007199254740993',b['dot'])
        self.assertIn('rankdir=TB',b['dot'])
        c=s.build_sample(core,snap,snap,stage,{**req,'depth':8,'collapsed':[1]})
        self.assertEqual(c['stats']['ripen_inputs'],1)
        self.assertEqual(c['stats']['visible_facts'],1)
    def test_budget_cycle_and_seed(self):
        core,snap,stage=fixture();snap['rows'][(1,(2,2))]=(1,(2,2,2),False)
        req={'focus':'all','sort':'Math','seed':12,'depth':8,'budget':1}
        a=s.build_sample(core,snap,snap,stage,req);b=s.build_sample(core,snap,snap,stage,req)
        self.assertEqual(a['root'],b['root']);self.assertEqual(a['dot'],b['dot'])
        self.assertLessEqual(a['stats']['visible_facts'],1)
    def test_no_future_and_subsumed_visibility(self):
        core,before,stage=fixture();after={**before,'rows':dict(before['rows'])}
        after['rows'][(1,(2,1))]=(1,(2,1,2),False)
        a=s.build_sample(core,before,after,stage,{'root':2,'sort':'Math','focus':'all','side':'before','context':False})
        b=s.build_sample(core,before,after,stage,{'root':2,'sort':'Math','focus':'all','side':'after','context':False})
        self.assertEqual(a['stats']['changed_in_view'],0);self.assertEqual(b['stats']['changed_in_view'],1)
        before['rows'][(1,(1,1))]=(1,(1,1,2),True)
        self.assertTrue(s.build_sample(core,before,before,stage,{'root':2,'sort':'Math','focus':'all','hidden':False})['empty'])
    def test_missing_locked_root_does_not_resample(self):
        core,snap,stage=fixture()
        self.assertTrue(s.build_sample(core,snap,snap,stage,{'root':999,'focus':'all'})['empty'])
    def test_union_renumber_is_not_new_structure(self):
        core,before,stage=fixture();values=before['values']+[{'sort':'Math','eq':'Value(3)'}]
        before={**before,'values':values,'rows':{(0,(0,)):(0,(0,3),False)}}
        after={**before,'aliases':{3:1},'rows':{(0,(0,)):(0,(0,1),False)}}
        result=s.build_sample(core,before,after,stage,{'root':3,'sort':'Math','focus':'all','side':'after'})
        self.assertEqual(result['root'],1);self.assertEqual(result['stats']['changed_in_view'],0)
    def test_initial_reads_and_safe_paths(self):
        core,snap,stage=fixture();stage={**stage,'ordinal':0,'ruleset':None}
        self.assertFalse(s.build_sample(core,snap,snap,stage,{'focus':'reads','sort':'Math'})['empty'])
        with self.assertRaises(ValueError):s.safe_folder('../../')
    def test_replay_backwards_and_partial_tail(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);core,snap,stage=fixture();(p/'manifest.json').write_text(json.dumps({'core':core}))
            data=[{'frame':0,'value_start':0,'values':snap['values'],'changes':[{'table':0,'removed':[],'upserts':[[0,1,0]]}],'aliases':[]}, {'frame':1,'value_start':3,'values':[],'changes':[{'table':1,'removed':[],'upserts':[[1,1,2,0]]}],'aliases':[]}]
            (p/'frames.ndjson').write_text(''.join(json.dumps(j)+'\n' for j in data)+'{"incomplete":')
            reader=s.Archive(p);self.assertEqual(len(reader.seek(1)['rows']),2);self.assertEqual(len(reader.seek(0)['rows']),1)
            self.assertEqual(len(s.lines(p/'frames.ndjson')),2)

if __name__=='__main__':unittest.main()

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

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

    def test_filter_include_all_any_exclude_and_root_are_independent(self):
        core,snap,stage=fixture();req={'sort':'Math','focus':'all','context':False,'depth':2}
        filters={'root_tables':['Add'],'include':['Input','Add'],'include_mode':'all'}
        a=s.build_sample(core,snap,snap,stage,{**req,'filters':filters})
        self.assertEqual(a['available_roots'],1);self.assertEqual(a['root'],2)
        self.assertFalse(a['filter_report']['selected_failures'])
        b=s.build_sample(core,snap,snap,stage,{**req,'filters':{'include':['Missing','Input'],'include_mode':'any'}})
        self.assertEqual(b['available_roots'],2)
        c=s.build_sample(core,snap,snap,stage,{**req,'filters':{**filters,'exclude':['Input']}})
        self.assertTrue(c['empty']);self.assertIn('排除',c['message'])
        self.assertEqual([x['remaining'] for x in c['filter_report']['steps']],[1,1,0])

    def test_filters_respect_display_cut_and_zero_upper_bound(self):
        core,snap,stage=fixture();req={'sort':'Math','focus':'all','context':False,'depth':0}
        a=s.build_sample(core,snap,snap,stage,{**req,'filters':{'root_tables':['Add'],'include':['Input']}})
        self.assertTrue(a['empty'])  # Input is hidden by RipenInput, not visible.
        b=s.build_sample(core,snap,snap,stage,{**req,'filters':{'max_ports':0}})
        self.assertEqual(b['root'],1);self.assertEqual(b['available_roots'],1)
        with self.assertRaises(ValueError):s.build_sample(core,snap,snap,stage,{**req,'filters':{'min_facts':3,'max_facts':2}})

    def test_shared_parents_are_not_repeated_positions(self):
        core,snap,stage=fixture();req={'sort':'Math','focus':'all','context':False,'depth':0}
        a=s.build_sample(core,snap,snap,stage,{**req,'filters':{'min_parents':2}})
        self.assertTrue(a['empty'])
        b=s.build_sample(core,snap,snap,stage,{**req,'filters':{'repeated_arg':True,'shared_ports':True}})
        self.assertEqual(b['root'],2)
        witness=b['filter_report']['selected']['sharing'][0]
        self.assertEqual(witness['parents'],1);self.assertEqual(witness['references'],2)
        snap['rows'][(1,(2,1))]=(1,(2,1,2),False)
        c=s.build_sample(core,snap,snap,stage,{**req,'filters':{'min_parents':2}})
        self.assertEqual(c['root'],2)

    def test_equivalence_filter_uses_real_constructor_members(self):
        core,snap,stage=fixture();snap['rows'][(1,(2,1))]=(1,(2,1,2),False)
        a=s.build_sample(core,snap,snap,stage,{'focus':'all','context':False,'alternatives':1,'depth':0,'filters':{'min_alternatives':2}})
        self.assertEqual(a['root'],2);self.assertEqual(a['stats']['visible_facts'],1)
        self.assertEqual(a['filter_report']['selected']['alternatives'],[{'class':2,'enodes':2,'shown':1}])

    def test_stage_change_filters_and_aliases(self):
        core,before,stage=fixture();after={**before,'rows':dict(before['rows']),'frame':1}
        after['rows'][(1,(1,1))]=(1,(1,1,2),True)
        req={'focus':'all','context':False,'hidden':True,'filters':{'change':'updated'}}
        a=s.build_sample(core,before,after,stage,req)
        self.assertEqual(a['root'],2)
        removed={**before,'rows':{k:v for k,v in before['rows'].items() if k[0]==0},'frame':2}
        a=s.build_sample(core,before,removed,stage,{'focus':'all','context':False,'side':'before','filters':{'change':'removed'}})
        self.assertEqual(a['root'],2)
        union={**before,'rows':{(0,(0,)):(0,(0,1),False)},'aliases':{2:1},'frame':3}
        a=s.build_sample(core,before,union,stage,{'focus':'all','context':False,'filters':{'change':'union'}})
        self.assertEqual(a['root'],1);self.assertEqual(a['filter_report']['selected']['unions'],[1])
        b=s.build_sample(core,before,union,stage,{'focus':'all','context':False,'filters':{'change':'unchanged'}})
        self.assertTrue(b['empty'])

    def test_fixed_root_and_manual_expansion_do_not_silently_resample(self):
        core,snap,stage=fixture();req={'focus':'all','context':False,'depth':0,'filters':{'include':['Add'],'exclude':['Input']}}
        a=s.build_sample(core,snap,snap,stage,req)
        b=s.build_sample(core,snap,snap,stage,{**req,'root':2,'expanded':[1]})
        self.assertEqual(a['available_roots'],b['available_roots']);self.assertEqual(b['root'],2)
        self.assertEqual(b['filter_report']['selected_failures'],['排除：Input'])
        c=s.build_sample(core,snap,snap,stage,{**req,'root':1})
        self.assertEqual(c['root'],1);self.assertFalse(c['filter_report']['selected_eligible'])

    def test_candidate_cache_and_exact_seed_selection(self):
        core,snap,stage=fixture();cache={};req={'focus':'all','context':False,'filters':{'min_facts':1}}
        with mock.patch.object(s.subprocess,'run',wraps=s.subprocess.run) as dot:
            a=s.build_sample(core,snap,snap,stage,{**req,'seed':13},cache)
            b=s.build_sample(core,snap,snap,stage,{**req,'seed':13},cache)
            self.assertEqual(dot.call_count,2)  # Only selected graphs are rendered.
        self.assertEqual(a['root'],b['root']);self.assertEqual(a['dot'],b['dot'])
        self.assertFalse(a['filter_report']['cached']);self.assertTrue(b['filter_report']['cached'])
        c=s.build_sample(core,snap,snap,stage,{**req,'filters':{'min_facts':2}},cache)
        self.assertFalse(c['filter_report']['cached']);self.assertEqual(c['available_roots'],1)

if __name__=='__main__':unittest.main()

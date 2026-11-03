import copy
import unittest
from dynamic import DynamicContext

N={'name':'N','kind':'eq'};P={'name':'Proof','kind':'eq'}
def var(name,sort=N):return {'var':name,'sort':sort}
def value(name):return {'sort':'N','eq':name}
def row(table,*values,hidden=False):return {'table':table,'values':[value(v) for v in values],'subsumed':hidden}
def core(repeated=False):
    tables=[{'name':n,'kind':'constructor','schema':[N]} for n in ['Leaf','Base','Other']]
    tables += [{'name':'Pair','kind':'constructor','schema':[N,N,N]},
               {'name':'Ready','kind':'constructor','schema':[N,P]}]
    rules=[]
    for name in ['Leaf','Base']:
        rules.append({'name':name,'body':[{'call':{'table':name},'args':[var('x')]}],
                      'head':[{'op':'let','dst':'p','sort':P,'call':{'table':'Ready'},'args':[var('x')]}]})
    if repeated:rules.append({'name':'same','body':[{'call':{'table':'Pair'},'args':[var('a'),var('a'),var('x')]}],
                       'head':[{'op':'let','dst':'p','sort':P,'call':{'table':'Ready'},'args':[var('x')]}]})
    return {'tables':tables,'rulesets':[{'name':'r','rules':rules}],'initial':[]}

class Dynamics(unittest.TestCase):
    def test_shared_support_survives_one_retirement(self):
        d=DynamicContext(core(),'r');d.apply(upserts=[(1,row('Leaf','a')),(2,row('Base','a'))])
        p=d.apply(removes=[1]);self.assertIsNone(p['words']);self.assertEqual(p['stats']['live_contexts'],1)
        p=d.apply(removes=[2]);self.assertEqual(p['stats']['contexts_retired'],1);self.assertEqual(d.banks[0],[])

    def test_union_revisits_previously_failed_anchor(self):
        d=DynamicContext(core(True),'r');d.apply(upserts=[(1,row('Pair','a','b','r'))])
        self.assertEqual(sum(map(len,d.banks.values())),0)
        p=d.apply(aliases=[('N','b','a')]);self.assertEqual(p['stats']['contexts_added'],1)
        self.assertEqual(p['stats']['anchors_regrounded'],1)

    def test_swap_removal_and_reuse_are_dense(self):
        d=DynamicContext(core(),'r');d.apply(upserts=[(i,row('Leaf',str(i))) for i in range(3)])
        p=d.apply(removes=[1],upserts=[(4,row('Leaf','new'))])
        self.assertEqual(len(d.banks[0]),3)
        for slot,key in enumerate(d.banks[0]):self.assertEqual(d.locations[key],(0,slot))
        self.assertEqual(p['stats']['contexts_retired'],1);self.assertEqual(p['stats']['contexts_added'],1)

    def test_stale_epoch_and_malformed_rows_are_atomic(self):
        d=DynamicContext(core(),'r');d.apply(upserts=[(1,row('Leaf','a'))]);before=copy.deepcopy(d.__dict__)
        with self.assertRaisesRegex(ValueError,'stale'):d.apply(removes=[1],expected_epoch=0)
        with self.assertRaisesRegex(ValueError,'arity'):d.apply(removes=[1],upserts=[(2,row('Leaf','x','y'))])
        self.assertEqual(d.epoch,before['epoch']);self.assertEqual(d.rows,before['rows']);self.assertEqual(d.banks,before['banks'])

    def test_unrelated_changes_do_not_invalidate_contexts(self):
        d=DynamicContext(core(),'r');d.apply(upserts=[(1,row('Leaf','a'))]);old=d.epoch
        p=d.apply(upserts=[(2,row('Other','b'))],aliases=[('Unrelated','x','y')])
        self.assertIsNone(p['words']);self.assertEqual(d.epoch,old)

    def test_replacement_does_not_accumulate_context_history(self):
        c=core();c['tables'][0]['kind']='function'
        d=DynamicContext(c,'r')
        for i in range(1000):d.apply(upserts=[(1,row('Leaf',str(i%2)))])
        self.assertEqual(len(d.rows),1);self.assertEqual(len(d.watch),1)
        self.assertEqual(len(d.supports),1);self.assertEqual(len(d.locations),1)
        self.assertEqual(sum(map(len,d.banks.values())),1);self.assertEqual(len(d.coordinates),2)

    def test_aliases_merge_fact_coordinates_without_reading_truth(self):
        d=DynamicContext(core(),'r');d.apply(upserts=[(1,row('Leaf','a')),(2,row('Leaf','b'))])
        p=d.apply(aliases=[('N','b','a')]);self.assertEqual(p['stats']['fact_merges'],1)
        self.assertEqual(sum(map(len,d.banks.values())),1)
        self.assertEqual(d.fact_root(0),d.fact_root(1))

if __name__=='__main__':unittest.main()

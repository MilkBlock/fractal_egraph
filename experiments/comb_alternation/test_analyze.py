import unittest
from analyze import category, extract, summarize

class AnalysisTests(unittest.TestCase):
    def test_equal_multiset_and_run_classification(self):
        self.assertEqual(category(['A','A','B','B']),'blocked')
        self.assertEqual(category(['A','B','A','B']),'alternating')
        self.assertEqual(category(['A','B','B','A']),'other')
        self.assertEqual(category(['A']*4),'repeat_only')
        self.assertEqual(category(['A','B','A','C']),'other')

    def test_paths_require_edges_and_deduplicate_parallel_support(self):
        events=[{'id':i,'scope':0,'round':i+1,'rule':r,'bindings':{'x':str(i)},'productive':True} for i,r in enumerate(['A','B','A','B'])]
        witnesses=[]
        for c in range(1,4):
            support={'producer':c-1,'table':'T','producer_sites':['head/0'],'consumer_sites':['body/0'],'kind':'row','write':c}
            witnesses.append({'consumer':c,'support':[support,dict(support,write=c+10)]})
        p={'events':events,'witnesses':witnesses}
        rows=extract(p,4)
        self.assertEqual(len(rows),1)
        self.assertEqual(rows[0]['category'],'alternating')
        self.assertEqual(len(rows[0]['writes']),6)
        p['witnesses'].pop(1)
        self.assertEqual(extract(p,4),[])

    def test_root_holdout_does_not_train_on_future_shape(self):
        row={'shape':'"s"','rules':['A','B','A','B'],'split':'train','cross_boundary':False,'root':4,'entry':1,'binding':'x','productive':True,'path':[1,2,3,4],'writes':[1,2,3],'external_producers':0}
        future=dict(row,shape='"new"',split='test',cross_boundary=True,root=5)
        r=summarize([row,future],1)
        self.assertEqual(r['test_hits']['paths'],0)
        self.assertEqual(r['rule_names_only_ablation']['test_hits']['paths'],1)
        self.assertEqual(r['cross_boundary_paths'],1)

if __name__=='__main__': unittest.main()

import copy
import unittest
import numpy as np
from features import View, labels, stage_spans
from study import fit_model, counts


def eq(name): return {'name': name, 'kind': 'eq'}
def value(name, sort='Expr'): return {'sort': sort, 'eq': name}
def row(name, *values): return {'table': name, 'values': list(values), 'subsumed': False}
def table(name, sorts, kind='constructor'):
    return {'name': name, 'schema': [eq(s) for s in sorts], 'kind': kind, 'internal': False}


def core():
    return {'tables': [table('Pair',['Expr','Expr','Expr']), table('Leaf',['Expr']),
                       table('HasType',['Expr','Type','___HasTypeSort']),table('Int',['Type']),
                       table('ExprIsPure',['Expr','___ExprIsPureSort'])],
            'rulesets': [{'name':'always-run','rules':[{'body':[{'call':{'table':'HasType'}}],
                         'head':[{'call':{'table':'ExprIsPure'}}]}]},
                        {'name':'type-analysis','rules':[{'body':[], 'head':[{'call':{'table':'HasType'}}]}]}]}


def snapshot():
    return {'rows':[row('Pair',value('a'),value('b'),value('p')),
                    row('Leaf',value('a')),row('Leaf',value('b')),row('Leaf',value('c')),
                    row('Int',value('t','Type')),
                    row('HasType',value('a'),value('t','Type'),value('private','___HasTypeSort'))], 'anchors':[]}


class PolyspineTests(unittest.TestCase):
    def test_cross_boundary_changes_without_local_summary_change(self):
        s=snapshot();v=View(core());a,_=v.extract(s,'always-run','type-analysis')
        s['rows'][-1]['values'][0]=value('c')
        b,_=v.extract(s,'always-run','type-analysis')
        get=lambda rows:next(x for x in rows if x[0][2]=='p')
        aa,bb=get(a),get(b)
        self.assertEqual(aa[1:4],bb[1:4])  # same root structure/stage/interfaces
        self.assertNotEqual(aa[4],bb[4])   # only the connected child is relevant

    def test_argument_roles_not_bagged_away(self):
        s=snapshot();v=View(core());a,_=v.extract(s,'always-run','type-analysis')
        s['rows'][0]['values'][:2]=list(reversed(s['rows'][0]['values'][:2]))
        b,_=v.extract(s,'always-run','type-analysis')
        get=lambda rows:next(x[4] for x in rows if x[0][2]=='p')
        self.assertNotEqual(get(a),get(b))

    def test_alpha_renaming(self):
        s=snapshot();v=View(core());a,_=v.extract(s,'always-run','type-analysis')
        for r in s['rows']:
            for x in r['values']: x['eq']='renamed-'+x['eq']
        b,_=v.extract(s,'always-run','type-analysis')
        self.assertEqual([x[1:] for x in a],[x[1:] for x in b])

    def test_target_private_identity_cannot_leak(self):
        s=snapshot();v=View(core());a,_=v.extract(s,'always-run','type-analysis')
        s['rows'].append(row('ExprIsPure',value('a'),value('secret','___ExprIsPureSort')))
        b,_=v.extract(s,'always-run','type-analysis')
        self.assertEqual([x[1:5] for x in a],[x[1:5] for x in b])
        self.assertNotEqual([x[5] for x in a],[x[5] for x in b])  # diagnostics only

    def test_subsumed_interface_excluded(self):
        s=snapshot();s['rows'][-1]['subsumed']=True
        rows,stats=View(core()).extract(s,'always-run','type-analysis')
        self.assertEqual(stats['seam_features'],0)

    def test_alias_preserved_and_post_alias_only_used_for_label(self):
        s=snapshot();s['rows'][0]['values'][1]=value('a')
        rows,_=View(core()).extract(s,'always-run','type-analysis')
        p=next(x for x in rows if x[0][2]=='p')
        self.assertIn(('alias','Pair',0,1),p[1])
        post={'anchors':[{'sort':'Expr','old':'p','now':'merged'}],
              'rows':[row('ExprIsPure',value('merged'),value('q','___ExprIsPureSort'))]}
        self.assertEqual(labels(post,[p[0]],'ExprIsPure'),[1])

    def test_schedule_saturation_not_run_chunk_heuristic(self):
        s=[{'saturate':{'sequence':[{'saturate':{'run':'type-helpers'}},{'run':'type-analysis'}]}}]
        trace=[{'set':'type-helpers','updated':False},{'set':'type-analysis','updated':True},
               {'set':'type-helpers','updated':False},{'set':'type-analysis','updated':False}]
        spans,n=stage_spans(s,trace)
        self.assertEqual(spans,[{'stage':'type-analysis','start':0,'end':4,'cycle':0}])
        with self.assertRaises(ValueError): stage_spans(s,trace[:-1])

    def test_aggregated_ridge_equals_expanded(self):
        x=np.array([[0,1],[1,0],[1,1]],dtype=np.float16)
        n=np.array([2,3,1]);pos=np.array([1,2,1])
        w,b,_=fit_model(x,n,pos)
        xx=np.repeat(x,n,axis=0).astype(float);y=np.array([1,0,1,1,0,1])
        mx=xx.mean(0);my=y.mean();z=xx-mx
        expected=np.linalg.solve(z.T@z+np.eye(2),z.T@(y-my))
        np.testing.assert_array_equal(w,(expected*2).astype(np.float16))
        self.assertAlmostEqual(float(b),my-mx@expected,places=6)

    def test_contradictory_labels_retained(self):
        x=np.zeros((1,4),np.float16);w,b,p=fit_model(x,np.array([4]),np.array([1]))
        self.assertEqual(p,.25);self.assertEqual(b,.25)
        self.assertEqual(counts(np.array([1,0]),np.array([.25,.25]),np.array([1,3]))['fn'],1)


if __name__=='__main__': unittest.main()

"""Independent finite enumeration checks for conditional optimization and ADDs."""
import itertools
import tempfile
import unittest
import copy
from pathlib import Path
from . import solver as gp
from .planner import solve
from .pricing import Diagram,RuntimePricing,cost,audit,rule_recipe
from . import markers,takeover


class TablePricing:
    """Small explicit test oracle; production always uses lowered runtime code."""
    def __init__(self,candidates,edges,weights=(1.,1.,1.),table=None):
        self.cs={c['id']:c for c in candidates};self.opts={o['id']:o for e in edges for o in e['options']}
        self.weights=weights;self.table=table;self.cache={}
    def optimize(self,plan):
        key=tuple(plan['selected']),tuple(plan['implementations'])
        if key in self.cache:return self.cache[key]
        domains=[self.cs[p].get('marker_layouts') or [dict(id=f'none_{p}',columns=0,lookup_atoms=0)] for p in plan['selected']]
        best=None
        for ls in itertools.product(*domains):
            if self.table:
                q,w=self.table(plan,ls)
            else:
                q=sum(self.opts[o]['search_atoms'] for o in plan['implementations'])
                w=sum(self.opts[o]['update_actions'] for o in plan['implementations'])
                for p,l in zip(plan['selected'],ls):
                    q+=l['lookup_atoms']*sum(sum(a['path']==p for a in self.opts[o].get('parents',[])) for o in plan['implementations'])
                    w+=l['columns']*sum(self.opts[o].get('target_path')==p for o in plan['implementations'])
            k=sum(l['columns'] for l in ls);value=self.weights[0]*k+self.weights[1]*q+self.weights[2]*w
            if best is None or value<best['value']:
                best=dict(value=value,marker_layouts=[l['id'] for l in ls],variables=0,constraints=0,
                          costs=dict(marker_columns=k,lookup_atoms_and_guards=q,write_atoms_and_unions=w))
        self.cache[key]=(None,best);return self.cache[key]


def solve_table(candidates,edges,output,weights=(1.,1.,1.)):
    return solve(candidates,edges,output,weights,pricing=TablePricing(candidates,edges,weights))


class ExactPricing(unittest.TestCase):
    def test_identical_conditional_models_share_an_optimum_but_guards_do_not(self):
        chosen={0:dict(marker_layouts=[dict(id='a',path=0,columns=1),dict(id='b',path=0,columns=2)])}
        def prepare(plan,folder):
            cs=copy.deepcopy(chosen)
            for l in cs[0]['marker_layouts']:l['id']+=str(plan['selected'][0]);l['path']=plan['selected'][0]
            rule=dict(name='rule-'+str(plan['selected'][0]),body=[],head=[],ruleset='r')
            if plan['selected']==[2]:rule['body']=[dict(kind='eq',args=[takeover.var('x'),takeover.var('y')])]
            return dict(chosen=cs,consumers=[rule])
        with tempfile.TemporaryDirectory() as d:
            oracle=RuntimePricing(prepare,{},Path(d),(1,1,1))
            _,first=oracle.optimize(dict(selected=[0],implementations=[]))
            _,second=oracle.optimize(dict(selected=[1],implementations=[]))
            self.assertEqual(first['value'],second['value']);self.assertEqual(second['marker_layouts'],['a1'])
            self.assertEqual(second['isomorphic_pricing_of'],first['folder']);self.assertEqual(oracle.equivalent_model_hits,1)
            _,third=oracle.optimize(dict(selected=[2],implementations=[]))
            self.assertNotIn('isomorphic_pricing_of',third);self.assertEqual(oracle.equivalent_model_hits,1)

    def test_recipe_cache_does_not_erase_guards_or_duplicate_syntax(self):
        fact=dict(kind='fact',expr=takeover.call('A',[takeover.var('x')]))
        r=dict(body=[fact],head=[])
        alias=dict(kind='eq',args=[takeover.var('x'),takeover.var('y')])
        self.assertNotEqual(rule_recipe(r),rule_recipe(dict(r,body=[fact,fact])))
        self.assertNotEqual(rule_recipe(r),rule_recipe(dict(r,body=[fact,alias])))

    def test_cost_audit_rejects_missing_added_or_changed_emitted_rule(self):
        r=dict(name='graft',body=[],head=[dict(kind='expr',expr=takeover.call('A!!',[]))])
        expected={'graft':cost(r)}
        audit([r],expected)
        for rules in ([],[r,r],[r,dict(r,name='extra')],[dict(r,head=[])]):
            with self.assertRaises(ValueError):audit(rules,expected)

    def test_identical_recipes_reuse_profiles_across_path_ids_and_variable_names(self):
        var,call=takeover.var,takeover.call
        chosen={}
        for i,op in enumerate(('A','B')):
            c=dict(id=i,family=i,roots=[call(op,[var('x')])],params={'x':'p0'},acts=[])
            c['marker_layouts']=markers.layouts(c);chosen[i]=c
        sig={op:dict(input=['T'],output='T') for op in ('A','B')}
        def rule(a,b):return dict(name='transition',ruleset='Run!',
            body=[dict(kind='fact',expr=call('Summary-0!',[var(a),var(b)]))],
            head=[dict(kind='expr',expr=call('Summary-1!',[call('B!',[var(b)]),var(b)]))])
        with tempfile.TemporaryDirectory() as tmp:
            oracle=RuntimePricing(None,sig,Path(tmp),(1,1,1))
            oracle.profile(rule('root','x'),chosen);n=oracle.evaluations
            renamed=copy.deepcopy(chosen)
            for c in renamed.values():
                for l in c['marker_layouts']:l['id']='renamed_'+l['id']
            r=rule('parent','value');root=oracle.profile(r,renamed)
            self.assertEqual(oracle.evaluations,n)
            for ls in itertools.product(*(c['marker_layouts'] for c in renamed.values())):
                with gp.Model() as model:
                    selected={l['id'] for l in ls}
                    variables={l['id']:model.addVar(vtype=gp.BINARY,lb=float(l['id'] in selected),ub=float(l['id'] in selected))
                               for c in renamed.values() for l in c['marker_layouts']}
                    expr,_=oracle.diagram.encode(model,variables,[root]);model.setObjective(expr[0]);model.optimize()
                    expected=cost(markers.lower_ast(r,dict(enumerate(ls)),sig))
                    self.assertEqual(model.ObjVal,expected['search_atoms']+expected['update_actions'])

    def test_inner_marker_result_can_reverse_the_outer_spine_choice(self):
        # Path 0 has cheaper shape storage, but no inexpensive joint layout.
        # Path 1 wins only when both endpoints choose the compatible marker.
        cs=[dict(id=0,family=0,nodes=['small'],marker_layouts=[dict(id='a0',columns=1),dict(id='a1',columns=2)]),
            dict(id=1,family=0,nodes=['wide-a','wide-b'],marker_layouts=[dict(id='b0',columns=1),dict(id='b1',columns=2)]),
            dict(id=2,family=1,nodes=['target'],marker_layouts=[dict(id='t0',columns=1),dict(id='t1',columns=2)])]
        edges=[dict(id=0,options=[dict(id='left',requires=[0,2]),dict(id='right',requires=[1,2])])]
        def table(plan,ls):
            return (0 if {l['id'] for l in ls}=={'b1','t1'} else 30),1
        pricing=TablePricing(cs,edges,table=table)
        with tempfile.TemporaryDirectory() as tmp:r=solve(cs,edges,Path(tmp),pricing=pricing)
        choices=[]
        for p in (0,1):
            for a,b in itertools.product(cs[p]['marker_layouts'],cs[2]['marker_layouts']):
                q,w=table(dict(selected=[p,2]),[a,b])
                choices.append((len(set(cs[p]['nodes']+cs[2]['nodes']))+a['columns']+b['columns']+q+w,p,a['id'],b['id']))
        expected=min(choices)
        self.assertEqual(r['objective'],expected[0]);self.assertEqual(r['bound'],expected[0])
        self.assertEqual(r['selected'],[1,2]);self.assertEqual(set(r['marker_layouts']),{'b1','t1'})
        self.assertEqual(r['priced_schemes'],2)

    def test_cost_diagram_matches_every_assignment_without_pair_binaries(self):
        d=Diagram();values={}
        # Cost depends jointly on both choices. Equal branches share a node.
        for i in range(4):
            values[i]=d.branch([(f'b{j}',d.leaf(2 if i==j else 9)) for j in range(4)])
        root=d.branch([(f'a{i}',values[i]) for i in range(4)])
        for i,j in itertools.product(range(4),repeat=2):
            with gp.Model() as m:
                variables={f'{s}{k}':m.addVar(vtype=gp.BINARY,lb=float(k==v),ub=float(k==v))
                           for s,v in [('a',i),('b',j)] for k in range(4)}
                expr,_=d.encode(m,variables,[root]);m.setObjective(expr[0]);m.optimize()
                self.assertEqual(m.ObjVal,2 if i==j else 9)
                self.assertEqual(m.NumBinVars,8)

    def test_marker_choice_is_not_a_local_minimum_per_cell(self):
        cs=[dict(id=i,family=i,nodes=[],marker_layouts=[dict(id=f'{i}-small',columns=1),dict(id=f'{i}-wide',columns=3)]) for i in range(2)]
        edges=[dict(id=0,options=[dict(id='graft',requires=[0,1])])]
        def table(plan,ls):return (0 if all(l['id'].endswith('wide') for l in ls) else 20),0
        oracle=TablePricing(cs,edges,table=table)
        with tempfile.TemporaryDirectory() as tmp:r=solve(cs,edges,Path(tmp),pricing=oracle)
        self.assertEqual(r['objective'],6);self.assertTrue(all(l.endswith('wide') for l in r['marker_layouts']))


if __name__=='__main__':unittest.main()

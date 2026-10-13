"""Exhaustive small-domain oracle for source reuse and layout projections."""
import itertools
import json
from pathlib import Path
import tempfile
import unittest

from . import markers,takeover as t
from .pricing import RuntimePricing,cost,families


def example(*,guarded=False,compound=False):
    chosen={}
    signatures={op:dict(input=['T']*n,output='T',kind='constructor')
                for op,n in [('A',1),('B',2),('C',2),('K',1)]}
    for f,(op,n) in enumerate([('A',1),('B',2),('C',2)]):
        root=t.call(op,[t.var(f'x{i}') for i in range(n)])
        c=dict(id=f,family=f,roots=[root],params={f'x{i}':f'p{i}' for i in range(n)},acts=[])
        if guarded and f==1:
            other=t.call('A',[t.var('x0')]);c['roots'].append(other)
            c['acts']=[dict(kind='union',args=[root,other])]
        ls=markers.layouts(c)
        # Exercise physical-table differences without asserting a raw-row
        # certificate: native tests independently establish those contracts.
        ls += [dict(l,id=l['id']+'_borrowed',representation='borrowed',read_storage='raw',stored_roots=[])
               for l in list(ls)]
        chosen[f]=dict(c,marker_layouts=ls)
    x=t.call('K!',[t.var('z')]) if compound else t.var('x')
    b_roots=[t.var('b')]+([t.var('b_alias')] if guarded else [])
    a=t.call('Summary-0!',[t.var('a'),x])
    b=t.call('Summary-1!',b_roots+[x,t.var('y')])
    target=t.call('Summary-2!',[t.call('C!',[x,t.var('y')]),x,t.var('y')])
    rule=dict(name='combine',ruleset='Run!',
        body=[dict(kind='fact',expr=a),dict(kind='fact',expr=b)],
        head=[dict(kind='expr',expr=target),dict(kind='change',change='subsume',head=a['head'],args=a['args'])])
    return chosen,signatures,rule


def diagram_value(diagram,root,selected):
    node=diagram.nodes[root]
    if 'value' in node:return node['value']
    branches=[child for child,labels in node['groups'] if selected.intersection(labels)]
    if len(branches)!=1:raise AssertionError('layout labels lost their one-hot partition')
    return diagram_value(diagram,branches[0],selected)


class LayoutPricing(unittest.TestCase):
    def test_all_small_layout_combinations_match_uncached_lowering(self):
        for guarded,compound in [(False,False),(True,True)]:
            chosen,sig,rule=example(guarded=guarded,compound=compound)
            scope=families(rule);sources=scope[:-1]
            with tempfile.TemporaryDirectory() as tmp:
                oracles=[]
                for mode in ('enumerate','source-cache','projected'):
                    oracle=RuntimePricing(None,sig,Path(tmp),(1,2,3),layout_pricing=mode)
                    root=oracle.profile(rule,chosen);oracles.append((oracle,root))
                cache={}
                for ls in itertools.product(*(chosen[f]['marker_layouts'] for f in scope)):
                    selected=dict(zip(scope,ls));key=tuple(selected[f]['id'] for f in sources)
                    if key not in cache:cache[key]=(markers.prepare_source(rule,selected,sig),{})
                    source,normalizations=cache[key];before=json.dumps(source,default=lambda v:sorted(v),sort_keys=True)
                    expected=markers.lower_ast(rule,selected,sig)
                    self.assertEqual(markers.resume_source(rule,selected,sig,source,normalizations=normalizations),expected)
                    self.assertEqual(json.dumps(source,default=lambda v:sorted(v),sort_keys=True),before)
                    c=cost(expected);value=2*c['search_atoms']+3*c['update_actions']
                    for oracle,root in oracles:
                        self.assertEqual(diagram_value(oracle.diagram,root,{l['id'] for l in ls}),value)
                self.assertLess(oracles[1][0].source_preparations,oracles[0][0].source_preparations)

    def test_projection_observes_each_use_and_preserves_original_labels(self):
        chosen,sig,rule=example()
        base=chosen[2]['marker_layouts'][0]
        changed=dict(base,id='same_create_different_decode',decode=[dict(kind='eq',guard=True,args=[t.var('p0'),t.var('p1')])])
        chosen[2]['marker_layouts'].append(changed)
        self.assertEqual(markers.layout_projection(base,{'create'}),markers.layout_projection(changed,{'create'}))
        self.assertNotEqual(markers.layout_projection(base,{'read'}),markers.layout_projection(changed,{'read'}))
        self.assertNotEqual(markers.layout_projection(base,{'read','create'}),markers.layout_projection(changed,{'read','create'}))
        borrowed=dict(base,representation='borrowed',read_storage='raw',stored_roots=[])
        self.assertEqual(markers.layout_projection(base,{'retire'}),markers.layout_projection(borrowed,{'retire'}))
        self.assertNotEqual(markers.layout_projection(base,{'read'}),markers.layout_projection(borrowed,{'read'}))
        altered=dict(base,terms=base['terms']+[t.var('p0')])
        self.assertNotEqual(markers.layout_projection(base,{'retire'}),markers.layout_projection(altered,{'retire'}))
        with tempfile.TemporaryDirectory() as tmp:
            oracle=RuntimePricing(None,sig,Path(tmp),(1,1,1));root=oracle.profile(rule,chosen)
            self.assertLess(oracle.evaluations,oracle.cartesian_tuples)
            self.assertLess(oracle.projected_choices,oracle.layout_choices)
            for ls in itertools.product(*(c['marker_layouts'] for c in chosen.values())):
                c=cost(markers.lower_ast(rule,dict(enumerate(ls)),sig))
                self.assertEqual(diagram_value(oracle.diagram,root,{l['id'] for l in ls}),c['search_atoms']+c['update_actions'])

    def test_read_and_write_same_family_and_no_owner(self):
        chosen,sig,rule=example(compound=True)
        a=rule['body'][0]['expr']
        for body in (rule['body'][:1],[]):
            r=dict(rule,body=body,head=[dict(kind='expr',expr=a)])
            for l in chosen[0]['marker_layouts']:
                selected={0:l};source=markers.prepare_source(r,selected,sig)
                self.assertEqual(markers.resume_source(r,selected,sig,source),markers.lower_ast(r,selected,sig))
        roles=markers.layout_roles(dict(rule,head=[dict(kind='expr',expr=a)]))
        self.assertEqual(roles[0],{'read','create'})


if __name__=='__main__':unittest.main()

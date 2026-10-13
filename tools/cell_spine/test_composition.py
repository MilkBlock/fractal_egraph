"""Certified seed cuts, including late bridges between already compact Cells."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from . import compiler as c,markers,takeover as t
from .composition import SeedCuts
from .terms import text


def input_term(i):return t.call('RipenInput',[dict(kind='literal',sort='i64',text=str(i))])


def candidate(identifier,family,roots,ports):
    sig={op:dict(input=['Term']*n,output='Term',kind='constructor') for op,n in [('A',1),('Join',2)]}
    sig['RipenInput']=dict(input=['i64'],output='Term',kind='constructor')
    values=[];rows=[];ids={}
    def add(e):
        key=text(e)
        if key in ids:return ids[key]
        args=[add(a) for a in e.get('args',[])]
        i=len(values);ids[key]=i
        values.append(dict(sort=e['sort'] if e['kind']=='literal' else sig[e['head']]['output'],literal=e['text'] if e['kind']=='literal' else None))
        if e['kind']=='call':rows.append(dict(op=e['head'],args=args,result=i))
        return i
    for root in roots:add(root)
    return dict(id=identifier,family=family,roots=roots,acts=[dict(kind='expr',expr=r) for r in roots],
        params={text(p):f'p{i}' for i,p in enumerate(ports)},param_sorts={text(p):'Term' for p in ports},
        signatures=sig,snapshot=dict(values=values,rows=rows),value_map=list(range(len(values))))


class SeedCutProofs(unittest.TestCase):
    def examples(self):
        p=input_term(7);a=input_term(11);b=input_term(12)
        parent=candidate(0,0,[t.call('A',[p])],[p])
        child=candidate(1,1,[t.call('Join',[t.call('A',[a]),t.call('A',[b])])],[a,b])
        return parent,child

    def test_generic_constructor_and_two_bindings_of_one_family(self):
        parent,child=self.examples();cuts=SeedCuts([parent,child]).cuts(child)
        self.assertEqual(len(cuts),1);self.assertEqual([p['parent'] for p in cuts[0]['parents']],[0,0])
        self.assertNotEqual(cuts[0]['parents'][0]['bindings'],cuts[0]['parents'][1]['bindings'])
        self.assertTrue(cuts[0]['boundary'].startswith('(Join '))

    def test_disconnected_roots_shared_child_and_self_family_do_not_invent_cut(self):
        p,candidate_=self.examples();q=copy.deepcopy(candidate_)
        q['roots']=q['roots'][0]['args'];q['acts']=[dict(kind='expr',expr=r) for r in q['roots']]
        self.assertEqual(SeedCuts([p,q]).cuts(q),[])
        q=copy.deepcopy(candidate_);q['roots'][0]['args'][1]=q['roots'][0]['args'][0]
        self.assertEqual(SeedCuts([p,q]).cuts(q),[])
        q=copy.deepcopy(candidate_);q['family']=p['family']
        self.assertEqual(SeedCuts([p,q]).cuts(q),[])

    def test_guards_and_port_types_are_not_erased(self):
        p,q=self.examples();p['acts'].append(dict(kind='union',args=[p['roots'][0],input_term(7)]))
        self.assertEqual(SeedCuts([p,q]).cuts(q),[])
        for root in q['roots'][0]['args']:q['acts'].append(dict(kind='union',args=[root,root['args'][0]]))
        self.assertEqual(len(SeedCuts([p,q]).cuts(q)),1)
        p['param_sorts'][text(input_term(7))]='Other'
        self.assertEqual(SeedCuts([p,q]).cuts(q),[])


class CompactMerge(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(dir=c.ROOT/'out',prefix='seed-cut-test-');cls.out=Path(cls.tmp.name)/'bake'
        cls.source=c.ROOT/'experiments/cell_spine/fixtures/add2_pair.egg'
        r=subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',cls.source,'--output',cls.out,
            '--cell-composition','seed-cuts','--skip-build'],cwd=c.ROOT,
            env=dict(os.environ,EGG_LAYOUT_CELL_WINDOWS='1',RAYON_NUM_THREADS='1'),capture_output=True,text=True)
        if r.returncode:raise AssertionError(r.stdout+r.stderr)
        cls.report=json.loads((cls.out/'report.json').read_text());cls.data=json.loads((cls.out/'work/input.json').read_text())
        ls=cls.report['retirement']['marker_layouts'];cls.small=min(ls,key=lambda f:len(ls[f]['fields']));cls.large=max(ls,key=lambda f:len(ls[f]['fields']))
        cls.layout=ls[cls.small]
        assert cls.layout.get('representation','canonical')=='canonical'

    @classmethod
    def tearDownClass(cls):cls.tmp.cleanup()

    def inputs(self,pairs):
        raw=[];compact=[]
        for name,x,y in pairs:
            raw.append(f'(let {name} (Add (Leaf "{x}") (Leaf "{y}")))')
            compact.append(f'(let {name}_a (Leaf! "{x}")) (let {name}_b (Leaf! "{y}")) (let {name} (Add!! {name}_a {name}_b))')
            fields=dict(zip(self.layout['fields'],[t.var(name),t.var(name+'_a'),t.var(name+'_b')]))
            args=[text(markers.canonical(t.replace(e,fields),self.data['signatures'])) for e in self.layout['terms']]
            compact.append(f'(Summary-{self.small}! '+ ' '.join(args)+')')
        return '\n'.join(raw)+'\n','\n'.join(compact)+'\n'

    def verify(self,label,pairs,bridges):
        reference,initial=self.inputs(pairs)
        compressed=initial
        for name,left,right in bridges:
            reference+=f'(let {name} (Add {left} {right}))\n(run-schedule (saturate algebra))\n'
            compressed+=f'(let {name} (Add! {left} {right}))\n'+markers.RUNTIME_SCHEDULE+'\n'
        reference+='(run-schedule (saturate algebra))'
        compressed+=markers.RUNTIME_SCHEDULE
        base=json.loads((self.out/'work/validation.json').read_text())
        base['cases']=[dict(name=label,reference='\n'.join(c.commands(self.data,{'declaration','rule'}))+'\n'+reference,
            compressed=compressed,restore='(run-schedule (saturate Export!))')]
        p=self.out/(label+'.json');p.write_text(json.dumps(base));dest=self.out/(label+'-result.json')
        r=subprocess.run([c.ROOT/'target/release/examples/spine_validate',p,dest],capture_output=True,text=True)
        self.assertEqual(r.returncode,0,r.stdout+r.stderr)
        return json.loads(dest.read_text())['cases'][0],compressed

    def test_late_bridge_uses_cut_and_retires_small_markers(self):
        self.assertGreater(self.report['cell_composition']['seed_cut_interfaces'],0)
        case,compressed=self.verify('late',[('left','a','b'),('right','c','d')],[('root','left','right')])
        self.assertEqual(case['comparison']['status'],'equivalent')
        self.assertFalse(case['original_rules_executed'])
        self.assertEqual(case['active_summary_tables'][f'Summary-{self.small}!'],0)
        self.assertEqual(case['active_summary_tables'][f'Summary-{self.large}!'],1)
        p=self.out/'late-input.egg';p.write_text(compressed)
        manifest=dict(constructors=sorted(self.data['signatures']),environment=str(self.out/'library.egg'),input=str(p),run=markers.RUNTIME_SCHEDULE,counts=True)
        # Put the bridge into Run so the per-rule diagnostics include its apply.
        initial=self.inputs([('left','a','b'),('right','c','d')])[1]+'(let root (Add! left right))'
        p.write_text(initial);m=self.out/'late-bench.json';m.write_text(json.dumps(manifest))
        r=subprocess.run([c.ROOT/'target/release/examples/native_math_baseline','bench-probes',m],capture_output=True,text=True,check=True)
        v=json.loads(r.stdout.splitlines()[-1])['verification']
        self.assertGreater(sum(n for k,n in v['rule_matches'].items() if k.startswith('cut_')),0)

    def test_no_bridge_does_not_merge(self):
        case,_=self.verify('unconnected',[('left','a','b'),('right','c','d')],[])
        self.assertEqual(case['active_summary_tables'][f'Summary-{self.small}!'],2)
        self.assertEqual(case['active_summary_tables'][f'Summary-{self.large}!'],0)

    def test_retired_small_cell_remains_usable_through_its_owner(self):
        case,_=self.verify('shared',[('left','a','b'),('right','c','d'),('other','e','f')],
            [('first','left','right'),('second','left','other')])
        self.assertEqual(case['active_summary_tables'][f'Summary-{self.large}!'],2)

    def test_one_instance_can_fill_two_runtime_ports(self):
        case,_=self.verify('alias',[('left','a','b')],[('root','left','left')])
        self.assertEqual(case['active_summary_tables'][f'Summary-{self.large}!'],1)


if __name__=='__main__':unittest.main()

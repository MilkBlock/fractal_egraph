import unittest
from . import lhs_coverage as lc, takeover as t
from .test_composition import input_term
from .terms import text


def fixture(guards=False):
    sig={op:dict(kind='constructor',input=['T']*n,output='T') for op,n in [('P',1),('Q',1),('Gate',1),('Out',1),('Pair',2)]}
    sig['RipenInput']=dict(kind='constructor',input=['i64'],output='T')
    p=input_term(0);q=input_term(1)
    values=[dict(sort='T',literal=None),dict(sort='T',literal=None),dict(sort='i64',literal='0'),dict(sort='i64',literal='1')]
    rows=[dict(op='RipenInput',args=[2],result=0),dict(op='RipenInput',args=[3],result=1)]
    for op,args in [('P',[0]),('Q',[0]),('Pair',[0,1])]+([('Gate',[0])] if guards else []):
        rows.append(dict(op=op,args=args,result=len(values)));values.append(dict(sort='T',literal=None))
    candidate=dict(id=0,family=0,acts=[dict(kind='expr',expr=t.call('P',[p]))],roots=[t.call('P',[p])],
        params={text(p):'p0'},param_sorts={text(p):'T'},markers={'RipenInput'},signatures=sig,
        boundary_positive=True,snapshot=dict(values=values,rows=rows,subsumed_rows=[],scope={'semantics':'positive-constructor-equality/v1'}),value_map=list(range(len(values))))
    x=t.var('x');y=t.var('y')
    fact=lambda op,args:dict(kind='fact',expr=t.call(op,args))
    rule=lambda name,body,head:dict(name=name,body=body,head=[dict(kind='expr',expr=head)])
    rules=[rule('back',[fact('Q',[x])],t.call('P',[x])),
           rule('guarded',[fact('Q',[x]),fact('Gate',[x])],t.call('Out',[x])),
           rule('repeat',[fact('Pair',[x,x])],t.call('P',[x])),
           rule('correlated',[fact('Pair',[x,y]),fact('Q',[y])],t.call('P',[x]))]
    data=dict(signatures=sig,rules=rules,commands=[{'kind':'rule'} for _ in rules])
    return candidate,data


class Coverage(unittest.TestCase):
    def test_new_entry_and_missing_fact_are_separate(self):
        c,data=fixture();records,entries,_=lc.Index(data).inspect(c)
        self.assertEqual(len(entries),1)
        self.assertEqual(entries[0][2]['rule'],'back')
        self.assertEqual(entries[0][2]['entry_facts'],['(Q (RipenInput 0))'])
        partial=next(r for r in records if r['rule']=='guarded')
        self.assertFalse(partial['lhs_complete'])
        self.assertEqual(partial['remaining_lhs'],['(Gate (RipenInput 0))'])

    def test_aliases_and_cross_fact_bindings_are_not_orred(self):
        c,data=fixture();records,_,_=lc.Index(data).inspect(c)
        self.assertFalse(any(r['rule']=='repeat' and r['lhs_complete'] for r in records))
        self.assertFalse(any(r['rule']=='correlated' and r['lhs_complete'] for r in records))

    def test_explicit_equality_is_an_entry_condition(self):
        c,data=fixture();p=input_term(0)
        # Q and P are equal in this particular closure; the incoming guard must
        # nevertheless remain in the entry, not be inferred from D at runtime.
        pr=next(r for r in c['snapshot']['rows'] if r['op']=='P');qr=next(r for r in c['snapshot']['rows'] if r['op']=='Q');qr['result']=pr['result']
        x=t.var('x');data['rules'][0]['body'].append(dict(kind='eq',args=[t.call('Q',[x]),t.call('P',[x])]))
        records,entries,_=lc.Index(data).inspect(c)
        # The LHS itself now includes P, so there is no broader entry.
        self.assertFalse(entries)
        self.assertTrue(any(r['rule']=='back' and r['lhs_complete'] for r in records))

    def test_primitive_and_union_aliased_ports_do_not_authorize_entries(self):
        c,data=fixture();data['rules'][0]['body'].append(dict(kind='fact',expr=t.call('>',[t.var('n'),dict(kind='literal',sort='i64',text='0')])))
        index=lc.Index(data);self.assertFalse(index.positive);self.assertFalse(index.inspect(c)[1])
        c,data=fixture();c['params'][text(input_term(1))]='p1';c['acts'].append(dict(kind='expr',expr=t.call('Pair',[input_term(0),input_term(1)])))
        next(r for r in c['snapshot']['rows'] if r['op']=='RipenInput' and r['args']==[3])['result']=0
        for row in c['snapshot']['rows']:
            if row['op']!='RipenInput':row['args']=[0 if a==1 else a for a in row['args']]
        self.assertIsNone(lc.project(c));self.assertFalse(lc.Index(data).inspect(c)[1])

    def test_rewrite_root_uses_the_matched_lhs_spelling(self):
        c,data=fixture();p=next(r for r in c['snapshot']['rows'] if r['op']=='P');q=next(r for r in c['snapshot']['rows'] if r['op']=='Q');q['result']=p['result']
        x=t.var('x');root=t.var('root')
        data['rules'][0]=dict(name='rewrite-back',body=[dict(kind='eq',args=[root,t.call('Q',[x])])],head=[dict(kind='union',args=[root,t.call('P',[x])])])
        _,entries,_=lc.Index(data).inspect(c)
        self.assertEqual(entries[0][2]['entry_facts'],['(Q (RipenInput 0))'])

    def test_nesting_literals_and_no_new_cell_families(self):
        c,data=fixture();p=input_term(0);graph=lc.Facts(c['snapshot'])
        self.assertTrue(list(graph.fact(dict(kind='fact',expr=t.call('RipenInput',[dict(kind='literal',sort='i64',text='0')])),{})))
        self.assertFalse(list(graph.fact(dict(kind='fact',expr=t.call('RipenInput',[dict(kind='literal',sort='i64',text='99')])),{})))
        edges=[];report=lc.extend(data,[c],[c],edges)
        self.assertEqual(report['new_entries'],1);self.assertEqual(len(edges),1)
        self.assertTrue(any(x['operator']=='Q' and x['positions'] for x in report['index']))
        from .knowledge import observations
        self.assertEqual(observations([c],edges,{0}),[])
        self.assertEqual(edges[0]['options'][0]['target_family'],0)
        self.assertEqual(len(c['snapshot']['rows']),5)
        off=[];lc.extend(data,[c],[c],off,emit=False);self.assertFalse(off)


class NativeTransfer(unittest.TestCase):
    def test_fixed_training_libraries_on_unseen_inputs(self):
        import json, subprocess, sys, tempfile
        from pathlib import Path
        from .compiler import ROOT
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='lhs-entry-test-') as d:
            out=Path(d)/'run'
            result=subprocess.run([sys.executable,ROOT/'experiments/cell_spine/lhs_coverage/probe.py','--output',out],cwd=ROOT,capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            data=json.loads((out/'results.json').read_text())['fixtures']
            for name,fixture in data.items():
                self.assertEqual(fixture['off']['cells'],fixture['entries']['cells'])
                self.assertEqual(fixture['off']['historical_paths'],fixture['entries']['historical_paths'])
                self.assertGreater(fixture['entries']['coverage']['emitted_entries'],0)
                self.assertTrue(all(c['complete_state_equal'] and c['original_rules_executed'] is False for c in fixture['entries']['checks']))
                if name!='math':self.assertTrue(any(not c['complete_state_equal'] for c in fixture['off']['checks']))


if __name__=='__main__':unittest.main()

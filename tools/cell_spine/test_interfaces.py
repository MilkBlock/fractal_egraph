"""Shared decoders must keep owner liveness, correlations and native union deltas."""
from pathlib import Path
import subprocess
import tempfile
import unittest

from . import interfaces, markers, takeover as t
from .compiler import ROOT, ast, compatible_program
from .pricing import RuntimePricing, cost
from .test_layout_pricing import diagram_value


def example():
    sig={op:dict(input=['T']*n,output='T',kind='constructor') for op,n in [('A',1),('B',2)]}
    sig['Leaf']=dict(input=['String'],output='T',kind='constructor')
    root=t.call('B',[t.call('A',[t.var('x')]),t.var('y')])
    c=dict(id=0,family=0,roots=[root],params={'x':'p0','y':'p1'},param_sorts={'x':'T','y':'T'},
           signatures=sig,acts=[dict(kind='expr',expr=root)])
    c['marker_layouts']=markers.layouts(c)[:1] # exact small domain for brute-force oracle
    logical=t.call('Summary-0!',map(t.var,['r0','p0','p1']))
    rule=dict(name='consume',ruleset='Run!',body=[dict(kind='fact',expr=logical)],
              head=[dict(kind='expr',expr=t.call('Seen!',logical['args']))])
    bundle=interfaces.extend(dict(chosen={0:c},consumers=[rule]))
    return bundle,sig


class InterfaceProofs(unittest.TestCase):
    def test_source_cannot_collide_with_generated_interface(self):
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'source.egg'
            path.write_text('(datatype T (Leaf) (Interface-0 T)) (ruleset R) (rewrite (Interface-0 x) x :ruleset R) (Interface-0 (Leaf)) (run-schedule (saturate R))')
            data=ast(path,Path(d)/'source.json')
            self.assertIn('reserved Cell namespace',compatible_program(data))

    def test_inline_is_selected_when_nothing_uses_the_view(self):
        bundle,sig=example();bundle['consumers']=bundle['consumers'][1:]
        with tempfile.TemporaryDirectory() as d:
            _,result=RuntimePricing(lambda *_:bundle,sig,Path(d),(1,1,1)).optimize(dict(selected=[0],implementations=[]))
            self.assertEqual(result['marker_layouts'],['m0_0'])
            self.assertEqual(result['rule_costs'],{})
            self.assertEqual(result['costs']['interface_columns'],0)

    def test_shared_read_keeps_live_marker_and_no_constructor_queries(self):
        bundle,sig=example();selected={0:bundle['chosen'][0]['marker_layouts'][1]}
        r=markers.lower_ast(bundle['consumers'][0],selected,sig)
        self.assertEqual(cost(r)['marker_reads'],1)
        self.assertEqual(cost(r)['interface_reads'],1)
        self.assertEqual(cost(r)['canonical_reads'],0)
        self.assertEqual(cost(r)['unions'],0)
        self.assertIn('(Summary-0! r0)',markers.render(r))

    def test_joint_pricing_matches_exhaustive_interface_choices(self):
        bundle,sig=example()
        # Distinct consumers share ONE maintenance rule and one stored view.
        first=bundle['consumers'][0]
        bundle['consumers']=[dict(first,name=f'consumer_{i}',head=[dict(kind='expr',expr=t.call(f'Seen{i}!',first['head'][0]['expr']['args']))])
                             for i in range(12)]+bundle['consumers'][1:]
        with tempfile.TemporaryDirectory() as d:
            oracle=RuntimePricing(lambda *_:bundle,sig,Path(d),(1,1,1))
            _,result=oracle.optimize(dict(selected=[0],implementations=[]))
            expected=[]
            for layout in bundle['chosen'][0]['marker_layouts']:
                rs=[markers.lower_ast(r,{0:layout},sig) for r in bundle['consumers']]
                expected.append(interfaces.storage_columns(layout)+sum(cost(r)['search_atoms']+cost(r)['update_actions'] for r in rs))
            self.assertEqual(result['value'],min(expected))
            self.assertEqual(result['marker_layouts'],['m0_0_shared'])
            self.assertEqual(len([n for n in result['rule_costs'] if n.startswith('interface_')]),1)
            self.assertEqual(result['costs']['interface_columns'],4)
            for r in bundle['consumers']:
                model=oracle.profile(r,bundle['chosen'])
                for layout in bundle['chosen'][0]['marker_layouts']:
                    lowered=markers.lower_ast(r,{0:layout},sig);c=cost(lowered)
                    self.assertEqual(diagram_value(oracle.diagram,model,{layout['id']}),c['search_atoms']+c['update_actions'])

    def test_native_retirement_union_and_binding_correlations(self):
        bundle,sig=example();selected={0:bundle['chosen'][0]['marker_layouts'][1]}
        lowered=[markers.lower_ast(r,selected,sig) for r in bundle['consumers']]
        producer=markers.render(lowered[-1]);consumer=markers.render(lowered[0])
        late=markers.render(dict(lowered[0],name='late',head=[dict(kind='expr',expr=t.call('Late!',map(t.var,['r0','p0','p1'])))]))
        program='''(datatype T! (Leaf! String) (A!! T!) (B!! T! T!))
(sort Cell!) (constructor Summary-0! (T!) Cell!)
(ruleset Run!) (relation Seen! (T! T! T!)) (relation Late! (T! T! T!))
'''+ '\n'.join(interfaces.declarations(selected,sig))+'\n'+producer+'\n'+consumer+'''
(let x (Leaf! "x")) (let y (Leaf! "y"))
(let z (Leaf! "z")) (let w (Leaf! "w"))
(let root (B!! (A!! x) y)) (Summary-0! root)
(run-schedule (saturate Run!))
(check (Seen! root x y))
(let other (B!! (A!! z) w)) (union root other)
(run-schedule (saturate Run!))
(check (Seen! root z w))
(fail (check (Seen! root x w)))
(subsume (Summary-0! root))
'''+late+'''
(run-schedule (saturate Run!))
(check (Interface-0! root x y (A!! x)))
(fail (check (Late! root x y)))
(fail (check (Late! root z w)))
'''
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'test.egg';path.write_text(program)
            run=subprocess.run([ROOT/'target/release/examples/native_math_baseline',path],capture_output=True,text=True)
            self.assertEqual(run.returncode,0,program+'\n'+run.stderr)

    def test_shared_producer_retains_guard_until_union_enables_it(self):
        bundle,sig=example();layout=bundle['chosen'][0]['marker_layouts'][1]
        layout=dict(layout,decode=layout['decode']+[dict(kind='eq',guard=True,args=[t.var('p0'),t.var('p1')])])
        selected={0:layout}
        lines=['(datatype T! (Leaf! String) (A!! T!) (B!! T! T!))',
               '(sort Cell!) (constructor Summary-0! (T!) Cell!) (ruleset Run!) (relation Seen! (T! T! T!))']
        lines+=interfaces.declarations(selected,sig)
        lines+=[markers.lower(r,selected,sig) for r in bundle['consumers']]
        lines.append('''(let x (Leaf! "x")) (let y (Leaf! "y"))
(let root (B!! (A!! x) y)) (Summary-0! root)
(run-schedule (saturate Run!))
(fail (check (Seen! root x y)))
(union x y)
(run-schedule (saturate Run!))
(check (Seen! root x y))''')
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'guard.egg';path.write_text('\n'.join(lines))
            run=subprocess.run([ROOT/'target/release/examples/native_math_baseline',path],capture_output=True,text=True)
            self.assertEqual(run.returncode,0,run.stderr)


if __name__=='__main__':unittest.main()

"""Published witnesses are a correlated ABI; empirical costs include failures."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from . import interfaces, markers, takeover as t
from .compiler import ROOT
from .pricing import RuntimePricing, cost
from .test_interfaces import example


class PublishedWitnesses(unittest.TestCase):
    def bundle(self):
        bundle,sig=example()
        bundle['consumers']=bundle['consumers'][:1]
        bundle['chosen'][0]['marker_layouts']=bundle['chosen'][0]['marker_layouts'][:1]
        return interfaces.extend(bundle,'publisher'),sig

    def test_publisher_is_an_option_not_an_unconditional_replacement(self):
        bundle,sig=self.bundle()
        ls=bundle['chosen'][0]['marker_layouts']
        self.assertEqual([l.get('interface','inline') for l in ls],['inline','published','shared'])
        direct=ls[1]
        self.assertEqual([t.text(e) for e in direct['terms']],['r0','p0','p1','(A! p0)'])
        for rule in bundle['consumers'][:1]:
            r=markers.lower_ast(rule,{0:direct},sig)
            self.assertEqual(cost(r)['canonical_reads'],0)
            self.assertEqual(cost(r)['interface_reads'],0)
            self.assertEqual(cost(r)['marker_reads'],1)
        self.assertTrue(markers.lower_ast(bundle['consumers'][-1],{0:direct},sig)['disabled'])
        self.assertIsNone(interfaces.published(dict(ls[0],representation='borrowed')))

    def test_guard_is_retained_and_publisher_can_reuse_intermediate_on_rhs(self):
        bundle,sig=self.bundle();l=bundle['chosen'][0]['marker_layouts'][1]
        l=dict(l,decode=[dict(kind='eq',guard=True,args=[t.var('p0'),t.var('p1')])])
        r=dict(bundle['consumers'][0],head=[dict(kind='expr',expr=t.call('Seen!',[t.call('A!',[t.var('p0')])]))])
        lowered=markers.lower_ast(r,{0:l},sig)
        self.assertEqual(cost(lowered)['canonical_reads'],0)
        # Equality normalization may encode the guard as repeated columns.
        owner=lowered['body'][0]['expr']['args']
        self.assertEqual(owner[1],owner[2])
        self.assertIn('(A! p0)',lowered['reused_subtrees'])

    def test_solver_selects_publication_for_repeated_subtree_consumers(self):
        bundle,sig=self.bundle();first=bundle['consumers'][0]
        bundle['consumers']=[dict(first,name=f'consumer_{i}',head=[dict(kind='expr',expr=t.call(f'Seen{i}!',
                              [t.call('A!',[t.var('p0')])]))]) for i in range(12)]+bundle['consumers'][1:]
        with tempfile.TemporaryDirectory() as d:
            _,result=RuntimePricing(lambda *_:bundle,sig,Path(d),(1,1,1)).optimize(dict(selected=[0],implementations=[]))
        self.assertEqual(result['marker_layouts'],['m0_0_published'])
        self.assertEqual(sum(r['canonical_reads'] for r in result['rule_costs'].values()),0)
        self.assertEqual(sum(r['canonical_writes'] for r in result['rule_costs'].values()),0)

    def test_native_birth_union_late_consumer_and_retirement(self):
        bundle,sig=self.bundle();layout=bundle['chosen'][0]['marker_layouts'][1]
        logical=t.call('Summary-0!',map(t.var,['r0','p0','p1']))
        birth=dict(name='birth',ruleset='Birth',body=[dict(kind='eq',args=[t.var('r0'),t.call('B!',[t.call('A!',[t.var('p0')]),t.var('p1')])])],
                   head=[dict(kind='expr',expr=logical)])
        consumer=bundle['consumers'][0]
        retire=dict(name='retire',ruleset='Retire',body=consumer['body'],
                    head=[dict(kind='change',change='subsume',head=logical['head'],args=logical['args'])])
        lines=['''(datatype T! (Leaf! String) (A! T!) (B! T! T!) (A!! T!) (B!! T! T!))
(sort Cell!) (constructor Summary-0! (T! T! T! T!) Cell!)
(ruleset Birth) (ruleset Run!) (ruleset Retire)
(relation Seen! (T! T! T!)) (relation Late! (T! T! T!))''']
        lines += [markers.lower(r,{0:layout},sig) for r in (birth,consumer,retire)]
        lines += ['''(let x (Leaf! "x")) (let y (Leaf! "y"))
(let z (Leaf! "z")) (let w (Leaf! "w"))
(let r (B! (A! x) y)) (let s (B! (A! z) w))
(run-schedule (saturate Birth)) (run-schedule (saturate Run!))
(check (Seen! r x y)) (check (Seen! s z w))
(union r s) (run-schedule (saturate Run!))
(check (Seen! r z w)) (fail (check (Seen! r x w)))
(union x z) (run-schedule (saturate Run!))
(check (Summary-0! r z y (A!! z)))
(run-schedule (saturate Retire))''']
        lines += [markers.lower(dict(consumer,name='late',head=[dict(kind='expr',expr=t.call('Late!',logical['args']))]),{0:layout},sig)]
        lines += ['(run-schedule (saturate Run!)) (fail (check (Late! r x y)))']
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'test.egg';p.write_text('\n'.join(lines))
            r=subprocess.run([ROOT/'target/release/examples/native_math_baseline',p],capture_output=True,text=True)
            self.assertEqual(r.returncode,0,p.read_text()+'\n'+r.stderr)


class NativeSearchSelection(unittest.TestCase):
    def test_search_and_storage_cuts_are_additive_not_double_counted(self):
        bundle=dict(chosen={0:dict(marker_layouts=[dict(id='a',columns=1),dict(id='b',columns=3)])},consumers=[])
        with tempfile.TemporaryDirectory() as d:
            p=RuntimePricing(lambda *_:bundle,{},Path(d),(1,1,1),
                storage=lambda b,s,f:dict(value=20 if s[0]['id']=='a' else 0),
                search=lambda b,s,f:dict(value=0 if s[0]['id']=='a' else 9))
            _,result=p.optimize(dict(selected=[0],implementations=[]))
            self.assertEqual(result['marker_layouts'],['b'])
            self.assertEqual(result['value'],12)

    def test_native_measurement_failure_does_not_fall_back_to_atom_count(self):
        bundle=dict(chosen={0:dict(marker_layouts=[dict(id='a',columns=1)])},consumers=[])
        def fail(*_):raise RuntimeError('native failure')
        with tempfile.TemporaryDirectory() as d:
            p=RuntimePricing(lambda *_:bundle,{},Path(d),(1,1,1),search=fail)
            with self.assertRaisesRegex(RuntimeError,'native failure'):
                p.optimize(dict(selected=[0],implementations=[]))

    def test_failed_search_reverses_layout_choice_and_is_cached(self):
        # Neither plan needs a successful consumer. Expensive unsuccessful
        # prefixes must nevertheless enter the solver's objective.
        bundle=dict(chosen={0:dict(marker_layouts=[dict(id='narrow',columns=1),dict(id='wide',columns=4)])},consumers=[])
        calls=[]
        def search(bundle,selected,folder):
            name=selected[0]['id'];calls.append(name)
            return dict(value=100 if name=='narrow' else 2,matches=0)
        with tempfile.TemporaryDirectory() as d:
            p=RuntimePricing(lambda *_:bundle,{},Path(d),(1,1,1),search=search)
            _,result=p.optimize(dict(selected=[0],implementations=[]))
            self.assertEqual(result['marker_layouts'],['wide'])
            self.assertEqual(result['value'],6)
            self.assertEqual(result['native_search']['matches'],0)
            p.optimize(dict(selected=[0],implementations=[]))
            self.assertEqual(len(calls),2)

    def test_native_run_counts_work_even_with_zero_matches(self):
        request=dict(environment='''(datatype T (Leaf i64) (P T) (Q T))
(ruleset S) (relation Answer (T))
(rule ((= x (P a)) (= x (Q b))) ((Answer x)) :ruleset S)''',
                     input='(P (Leaf 1)) (Q (Leaf 2))',run='(run-schedule (saturate S))',measure_search=True)
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'in.json';p.write_text(json.dumps(request))
            r=subprocess.run([ROOT/'target/release/examples/native_math_baseline','storage',p],capture_output=True,text=True,check=True)
            result=json.loads(r.stdout.splitlines()[-1])
            self.assertNotIn('Answer',result['retained']['tables'])
            self.assertGreater(result['run_probe_counters']['probes'],0)
            self.assertGreater(result['run_probe_counters']['candidate_rows'],0)

    def test_complete_native_bake_search_selection_and_restoration(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='publisher-pipeline-') as d:
            out=Path(d)/'bake'
            r=subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',
                ROOT/'experiments/cell_spine/published_markers/pipeline.egg','--output',out,
                '--cell-interfaces','publisher','--search-cost','native','--skip-build'],
                capture_output=True,text=True,cwd=ROOT)
            self.assertEqual(r.returncode,0,r.stdout+r.stderr)
            report=json.loads((out/'report.json').read_text())
            self.assertEqual(report['search_cost'],'native')
            self.assertEqual(report['plan']['status'],'optimal')
            sample=report['native_search'];c=sample['counters']
            self.assertEqual(sample['value'],c['probes']+c['candidate_rows'])
            cases=json.loads((out/'validation.json').read_text())['cases']
            for case in cases:
                self.assertEqual(case['comparison']['status'],'equivalent')
                self.assertEqual(case['original_constructor_rows'],case['restored_constructor_rows'])
            self.assertTrue((out/'runtime.egg').exists())


if __name__=='__main__':unittest.main()

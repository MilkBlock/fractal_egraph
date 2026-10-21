"""Source-level view invariants under mixed rules, exact Run boundaries and unions."""
from pathlib import Path
import subprocess
import tempfile
import unittest

from .compiler import ROOT
from .pass3_markers import compile_source,publications,canonical,flatten
from . import takeover as t


HEADER='''(datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))
(function Cost (Expr) i64 :merge new)
(relation Seen0 (Expr Expr)) (relation Seen1 (Expr Expr)) (relation Seen2 (Expr Expr))
(ruleset make) (ruleset use)
(rule ((= s (Single x))) ((union (Get s 0) x)) :ruleset make :name "publisher")
'''
for i in range(3):
    HEADER+=f'''(rule ((= s (Single x)) (= y (Get s 0)) (= c (Cost x)) (< c 10))
 ((Seen{i} x y)) :ruleset use :name "consumer{i}")\n'''


class Pass3Markers(unittest.TestCase):
    def test_reads_before_and_after_set_are_not_one_timeless_value(self):
        v,c=t.var,t.call
        sig={name:dict(kind='constructor',input=['Expr'],output='Expr') for name in ['A','B']}
        sig['Best']=dict(kind='function',input=['Expr'],output='Expr')
        view=dict(name='M',sorts=['Expr']*3,body=[dict(kind='eq',args=[v('p0'),c('A',[v('p1')])]),
                  dict(kind='eq',args=[v('p2'),c('B',[v('p1')])])])
        rule=dict(body=[],head=[dict(kind='let',name='before',expr=c('Best',[v('x')])),
                  dict(kind='set',head='Best',args=[v('x')],value=v('z')),
                  dict(kind='let',name='after',expr=c('Best',[v('x')])),
                  dict(kind='expr',expr=c('A',[v('before')])),dict(kind='expr',expr=c('B',[v('after')]))])
        self.assertEqual(publications(rule,[view],sig),[])

    def compile(self,source):
        self.temp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='source-markers-')
        self.addCleanup(self.temp.cleanup);root=Path(self.temp.name)
        path=root/'input.egg';path.write_text(source)
        r=compile_source(path,ROOT/'experiments/analyze_library_credit/library',root/'compiled')
        return path,r

    def execute(self,path):
        p=subprocess.run([ROOT/'target/release/examples/native_math_baseline',path],capture_output=True,text=True)
        self.assertEqual(p.returncode,0,p.stderr)

    def test_finite_run_late_fact_guard_update_union_and_correlated_binding(self):
        source=HEADER+'''
(let x (Leaf 1)) (let z (Leaf 2)) (let s (Single x))
(set (Cost x) 20)
(run make 1) (run use 1)
(fail (check (Seen0 x x)))
(set (Cost x) 5) (run use 1)
(check (Seen0 x x) (Seen1 x x) (Seen2 x x))
(union s (Single z)) (set (Cost z) 4)
(run use 1)
(check (Seen0 z x)) (fail (check (Seen0 z z)))
'''
        path,r=self.compile(source);self.assertTrue(r['markers'])
        self.execute(path)
        for v in r['variants'].values():self.execute(Path(v['path']))
        self.assertGreater(r['variants']['published']['publish_actions'],0)

    def test_subsumable_constructor_is_not_a_permanent_view_source(self):
        path,r=self.compile(HEADER+'(Single (Leaf 1)) (run make 1) (subsume (Single (Leaf 1))) (run use 1)')
        self.assertEqual(r['markers'],[])
        for v in r['variants'].values():self.execute(Path(v['path']))

    def test_effectful_numeric_values_stay_outside_marker(self):
        _,r=self.compile(HEADER+'(Single (Leaf 1)) (run-schedule (saturate (seq make use)))')
        for view in r['markers']:
            self.assertNotIn('Cost',str(view['body']))
        emitted=Path(r['variants']['published']['path']).read_text()
        self.assertIn('(Cost x)',emitted)
        self.assertIn('(< c 10)',emitted)

    def test_late_publisher_does_not_advance_other_rules_in_same_run(self):
        source=HEADER.replace(':ruleset make',':ruleset use')+'''
(let x (Leaf 1)) (Single x) (set (Cost x) 5)
(run use 1) (fail (check (Seen0 x x)))
(run use 1) (check (Seen0 x x))
'''
        path,r=self.compile(source);self.execute(path)
        for v in r['variants'].values():self.execute(Path(v['path']))

    def test_unscheduled_consumers_do_not_justify_a_marker(self):
        _,r=self.compile(HEADER+'(Single (Leaf 1)) (run make 1)')
        self.assertEqual(r['markers'],[])

    def test_combined_ruleset_members_are_counted(self):
        _,r=self.compile(HEADER+'(unstable-combined-ruleset all make use) (Single (Leaf 1)) (run all 1)')
        self.assertTrue(r['markers'])
        self.assertIn('use',r['scheduled_rulesets'])

    def test_deferred_installation_rejects_late_global_capture(self):
        source=HEADER+'(let x (Leaf 9)) (rule () ((Single (Leaf 10))) :ruleset make :name "late-rule") (run make 1) (run use 1)'
        with self.assertRaisesRegex(ValueError,'capture'):
            self.compile(source)

    def test_existing_global_is_a_value_not_an_alpha_renamable_slot(self):
        source=HEADER.replace('(ruleset make)','(let zzz (Leaf 99)) (ruleset make)').replace('(< c 10)','(< c 10) (= x zzz)')+'''
(let x (Leaf 1)) (Single x) (set (Cost x) 1)
(run make 1) (run use 1) (fail (check (Seen0 x x)))
'''
        path,r=self.compile(source);self.execute(path)
        for v in r['variants'].values():self.execute(Path(v['path']))


if __name__=='__main__':unittest.main()

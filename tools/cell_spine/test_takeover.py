"""Retirement must preserve late consumers without reviving smaller summaries."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from .markers import RUNTIME_SCHEDULE
from .compiler import ROOT
from .test_support import marker_pattern
from .takeover import ancestors, var, call, entailed, normalize


class Proofs(unittest.TestCase):
    def test_installed_support_equation_removes_only_proved_effect(self):
        root,x,y=map(var,('root','x','y'))
        a,b=call('A!',[x]),call('B!',[x])
        s=call('Summary-0!',[root,x])
        d={'Summary-0!':dict(fields=['root','x'],roots=[a],aliases=[],
                            materialized=[a,b],equalities=[[b,root]])}
        head=[dict(kind='union',args=[b,root]),dict(kind='union',args=[root,y]),dict(kind='expr',expr=s)]
        self.assertEqual(entailed([dict(kind='fact',expr=s)],head,d,[]),(True,[True,False]))
        d['Summary-0!']['equalities']=[]
        self.assertEqual(entailed([dict(kind='fact',expr=s)],head,d,[]),(True,[False,False]))

    def test_effect_order_and_different_instances(self):
        x,y=map(var,('x','y'));a,b=call('A!',[x]),call('A!',[y]);s=call('Summary-0!',[x])
        head=[dict(kind='expr',expr=a),dict(kind='expr',expr=a),dict(kind='union',args=[a,b]),
              dict(kind='union',args=[b,a]),dict(kind='expr',expr=s)]
        self.assertEqual(entailed([],head,{},[]),(False,[False,True,False,True]))

    def test_cyclic_dominance_is_not_retirement(self):
        lenses = [dict(owner=a,child=b,owner_fields=['p'],child_fields=['p'],args=[var('p')],evidence=[])
                  for a,b in [(0,1),(1,0),(2,1)]]
        safe, blocked = ancestors(lenses)
        self.assertEqual([(x['owner'],x['child']) for x in safe],[(2,1)])
        self.assertEqual(len(blocked),2)

    def test_existing_summary_does_not_prove_hidden_row_materialized(self):
        root, x = var('root'),var('x')
        s=call('Summary-0!',[root,x]); a=call('A!',[x])
        body=[dict(kind='fact',expr=s)]
        head=[dict(kind='expr',expr=call('Hidden!',[x])),dict(kind='expr',expr=s)]
        definitions={'Summary-0!':dict(fields=['root','x'],roots=[a],aliases=[],materialized=[a])}
        covered, implied = entailed(body,head,definitions,[])
        self.assertTrue(covered)
        self.assertEqual(implied,[False])

    def test_equal_constructor_results_do_not_imply_equal_arguments(self):
        root,a,b=map(var,('root','a','b'))
        s=call('Summary-0!',[root,a]); other=call('Summary-0!',[root,b])
        body=[dict(kind='fact',expr=e) for e in (s,other)]
        head=[dict(kind='union',args=[a,b]),dict(kind='expr',expr=s)]
        definitions={'Summary-0!':dict(fields=['root','a'],roots=[call('A!',[a])],aliases=[],materialized=[call('A!',[a])])}
        covered, implied=entailed(body,head,definitions,[])
        self.assertTrue(covered)
        self.assertEqual(implied,[False])

    def test_alpha_dedup_preserves_literals(self):
        def key(name,literal):
            term=call('A',[var(name),dict(kind='literal',text=literal,sort='i64')])
            return normalize([dict(kind='fact',expr=term)],[dict(kind='expr',expr=term)])[2]
        self.assertEqual(key('x','1'),key('y','1'))
        self.assertNotEqual(key('x','1'),key('x','2'))


class NativeTakeover(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='takeover-test-')
        cls.folder=Path(cls.tmp.name);cls.outputs={}
        for name in ('late_takeover','two_retired_parents','nested_parents','transitive_takeover'):
            out=cls.folder/name
            subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',
                ROOT/f'experiments/cell_spine/fixtures/{name}.egg','--output',out,'--mode','symbolic','--skip-build','--storage-cost','structural'],
                check=True,cwd=ROOT,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            cls.outputs[name]=out

    @classmethod
    def tearDownClass(cls): cls.tmp.cleanup()

    def verify(self, fixture, name, code, expected, remove_inherited=False, reference=None):
        out=self.outputs[fixture]
        for family,pattern in [(0,'(Summary-0! r x)'),(0,'(Summary-0! r a)'),(3,'(Summary-3! a b x)')]:
            if pattern in code:
                bindings={'r0':'(A! x)','p0':'x'} if pattern=='(Summary-0! r x)' else None
                code=code.replace(pattern,marker_pattern(out,family,bindings))
        manifest=json.loads((out/'work/validation.json').read_text())
        if remove_inherited:
            report=json.loads((out/'report.json').read_text())
            for t in report['transitions']:
                if t['inherited']:
                    for emitted in report['retirement']['runtime_rules'][t['id']]:
                        self.assertIn(emitted,manifest['library'])
                        manifest['library']=manifest['library'].replace(emitted,'')
        c=dict(name=name,reference='',reference_path=str(ROOT/f'experiments/cell_spine/fixtures/{fixture}.egg'),
            compressed=code,restore='(run-schedule (saturate Export!))',expected_active_summaries=expected)
        if reference is not None: c.update(reference=reference,reference_path=None)
        manifest['cases']=[c]
        path=self.folder/f'{name}.json';path.write_text(json.dumps(manifest))
        result=subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/f'{name}-result.json'],
                              capture_output=True,text=True)
        if remove_inherited:
            self.assertNotEqual(result.returncode,0)
            self.assertTrue('active Summary-' in result.stderr or 'full closure mismatch' in result.stderr,result.stderr)
            return
        self.assertEqual(result.returncode,0,result.stderr)
        return json.loads((self.folder/f'{name}-result.json').read_text())['cases'][0]

    def late_code(self, alias=False):
        q=f'(Q! y)\n{RUNTIME_SCHEDULE}\n(fail (check (Summary-3! a b x)))\n(union x y)' if alias else '(Q! x)'
        return f'''
(let x (Leaf! "x"))
''' + ('(let y (Leaf! "y"))\n' if alias else '') + f'''
(A! x) (P! x)
{RUNTIME_SCHEDULE}
; Install a real matcher AFTER retirement. check alone sees subsumed rows!
(relation SawRetired ())
(rule ((Summary-0! r x)) ((SawRetired)) :ruleset Run! :name "probe-retired")
''' + q + f'''
{RUNTIME_SCHEDULE}
(fail (check (SawRetired)))
'''

    def test_late_q_uses_large_cell_and_does_not_revive_small(self):
        expected={'Summary-0!':0,'Summary-1!':0,'Summary-2!':1,'Summary-3!':1}
        result=self.verify('late_takeover','late',self.late_code(),expected)
        self.assertGreater(result['subsumed_summary_rows'],0)
        self.assertGreater(sum(n for r,n in result['compressed_logical_matches_before_export'].items() if '_cover_' in r),0)
        # Without compiler-generated inherited consumers, the same late-Q run
        # cannot derive Other: no raw alternative is secretly saving this test.
        self.verify('late_takeover','late-disabled',self.late_code(),expected,remove_inherited=True)

    def test_union_after_retirement_enables_inherited_binding(self):
        source=(ROOT/'experiments/cell_spine/fixtures/late_takeover.egg').read_text()
        source=source.replace('(Q (Leaf "x"))','(Q (Leaf "y"))\n(union (Leaf "x") (Leaf "y"))')
        self.verify('late_takeover','late-union',self.late_code(True),
                    {'Summary-0!':0,'Summary-3!':1},reference=source)

    def test_uncovered_instance_of_same_family_remains_active(self):
        code='(A! (Leaf! "z"))\n'+self.late_code()
        source=(ROOT/'experiments/cell_spine/fixtures/late_takeover.egg').read_text()
        source=source.replace('(run-schedule','(A (Leaf "z"))\n(run-schedule',1)
        self.verify('late_takeover','uncovered-instance',code,{'Summary-0!':1},reference=source)

    def test_two_retired_instances_can_still_join_on_late_gate(self):
        code=f'''
(let x (Leaf! "x")) (let y (Leaf! "y"))
(A! x) (A! y) (P! x) (P! y)
{RUNTIME_SCHEDULE}
(relation SawRetired ())
(rule ((Summary-0! r a)) ((SawRetired)) :ruleset Run! :name "probe-retired")
(Gate! x y)
{RUNTIME_SCHEDULE}
(fail (check (SawRetired)))
'''
        r=self.verify('two_retired_parents','two-retired',code,{'Summary-0!':0})
        self.assertGreater(sum(n for name,n in r['compressed_logical_matches_before_export'].items() if '_cover_' in name),0)
        self.verify('two_retired_parents','two-disabled',code,{'Summary-0!':0},remove_inherited=True)

    def test_grandparent_consumer_survives_two_retirements(self):
        code=self.late_code().replace('(A! x) (P! x)', '(A! x) (P! x) (R! x)')
        result=self.verify('transitive_takeover','grandparent-late',code,{'Summary-0!':0})
        self.assertEqual(result['active_summary_rows'],2)
        self.verify('transitive_takeover','grandparent-disabled',code,{'Summary-0!':0},remove_inherited=True)

    def test_transitive_dominance_leaves_only_largest_summary_active(self):
        out=self.outputs['nested_parents']
        v=json.loads((out/'validation.json').read_text())['cases'][-1]
        self.assertEqual(v['active_summary_rows'],1)
        self.assertEqual(v['subsumed_summary_rows'],4)
        r=json.loads((out/'report.json').read_text())
        self.assertTrue(any(len(l['evidence'])>1 for l in r['retirement']['lenses']))
        self.assertGreater(r['retirement']['redundant_inherited_rules_removed'],0)


if __name__=='__main__': unittest.main()

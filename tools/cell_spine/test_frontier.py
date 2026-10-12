"""Native regressions for certified closure interfaces and phased value reuse."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from .compiler import ROOT, ast
from .frontier import RUNTIME_SCHEDULE, definitions, rewrite_body
from experiments.cell_spine.reuse_boundary.probe import HEADER, tree


class NativeFrontier(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='frontier-test-')
        cls.folder=Path(cls.tmp.name);out=cls.folder/'bake'
        subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',
            ROOT/'experiments/cell_spine/fixtures/assoc.egg','--output',out,'--skip-build'],
            cwd=ROOT,check=True,capture_output=True)
        cls.lib=(out/'library.egg').read_text()
        cls.report=json.loads((out/'report.json').read_text())

    @classmethod
    def tearDownClass(cls): cls.tmp.cleanup()

    def verify(self, name, cases, lib=None, valid=True):
        path=self.folder/(name+'.json');out=self.folder/(name+'-result.json')
        path.write_text(json.dumps(dict(constructors=['Leaf','Plus'],library=self.lib if lib is None else lib,cases=cases)))
        r=subprocess.run([ROOT/'target/release/examples/spine_validate',path,out],capture_output=True,text=True)
        if not valid:
            self.assertNotEqual(r.returncode,0)
            self.assertIn('full closure mismatch',r.stderr)
            return
        self.assertEqual(r.returncode,0,r.stderr)
        return json.loads(out.read_text())['cases']

    def case(self,n,left):
        return dict(name=f'{n}-{left}',reference=HEADER+tree(n,left)+'\n(run-schedule (saturate algebra))',
                    compressed=tree(n,left,'Spine-')+'\n'+RUNTIME_SCHEDULE,
                    restore='(run-schedule (saturate Spine-Export))')

    def test_frozen_library_closes_six_new_trees_without_original_rules(self):
        cases=[self.case(n,left) for n in (3,4,7) for left in (True,False)]
        results=self.verify('trees',cases)
        for c,n in zip(results,(15,15,54,54,1939,1939)):
            self.assertEqual(c['original_constructor_rows'],n)
            self.assertEqual(c['restored_constructor_rows'],n)
            self.assertEqual(c['original_constructor_eclasses'],c['restored_constructor_eclasses'])
            self.assertEqual(c['comparison']['status'],'equivalent')
            self.assertFalse(c['original_rules_executed'])
            self.assertEqual(c['physical_original_rows_before_export'],0)
            # Include every auxiliary table: constructor reduction is not a
            # claim of total storage reduction.
            tables=c['native_tables_before_export']
            self.assertEqual(sum(tables.values()),c['native_total_rows_before_export'])
            self.assertIn('Spine-Read-Plus',tables)
            self.assertIn('Spine-Wanted-Plus',tables)
        self.assertLess(results[-1]['spine_constructor_rows'],1939)

    def test_disabling_certified_publications_reproduces_missing_closure(self):
        lib=self.lib
        for name,rules in self.report['retirement']['runtime_rules'].items():
            if name.startswith('frontier_'):
                for rule in rules:
                    self.assertIn(rule,lib)
                    lib=lib.replace(rule,'')
        self.verify('no-interface',[self.case(3,False)],lib,False)

    def test_overlapping_inputs_and_late_union_keep_native_identity(self):
        initial=tree(4,False)
        ref=HEADER+initial+'\n(run-schedule (saturate algebra))\n(union (Leaf "0") (Leaf "1"))\n(run-schedule (saturate algebra))'
        spine=tree(4,False,'Spine-')+'\n'+RUNTIME_SCHEDULE+'\n(union (Spine-Leaf "0") (Spine-Leaf "1"))\n'+RUNTIME_SCHEDULE
        self.verify('late-union',[dict(name='late-union',reference=ref,compressed=spine,restore='(run-schedule (saturate Spine-Export))')])

    def test_guards_and_known_results_survive_rule_lowering(self):
        # No algebraic operator or law is involved. The read view supplies a
        # result without its constructor, and a primitive guard rejects x=9.
        src=self.folder/'guards.egg'
        src.write_text('''(datatype Term (K i64))
(datatype Spine-Term (Spine-K i64) (Handle i64))
(relation Seen (Spine-Term))
(relation Trigger (i64))
(ruleset Spine-Run)
(rule ((= v (Spine-K x)) (> x 10)) ((Seen (Spine-K x))) :ruleset Spine-Run :name "guarded")
(rule ((Trigger x) (> x 10)) ((Seen (Spine-K x))) :ruleset Spine-Run :name "requested")
''')
        data=ast(src,self.folder/'guards.json')
        source='\n'.join(c['text'] for c in data['commands'] if c['kind']=='declaration')+'\n'
        source+='\n'.join(definitions({'K'},{'K':dict(input=['i64'],output='Term')}))+'\n'
        source+='\n'.join(line for rule in data['rules'] for line in rewrite_body(rule,{'K'}))+'\n'
        source+='''(Spine-Read-K 9 (Handle 9)) (Spine-Read-K 11 (Handle 11))
(Trigger 9) (Trigger 11) (Trigger 12)
'''+RUNTIME_SCHEDULE+'''
(check (Seen (Handle 11)))
(fail (check (Seen (Handle 9))))
(fail (check (Spine-K 11)))
(fail (check (Spine-K 9)))
(check (Seen (Spine-K 12)))
'''
        src.write_text(source)
        r=subprocess.run([ROOT/'target/release/examples/native_math_baseline',src],capture_output=True,text=True)
        self.assertEqual(r.returncode,0,r.stderr)


if __name__=='__main__':unittest.main()

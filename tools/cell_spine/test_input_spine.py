"""Direct input projection: selected storage, live boundary values and export."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from .compiler import ROOT,cellify
from .input_spine import project
from .markers import RUNTIME_SCHEDULE


class InputProjection(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='input-spine-test-')
        cls.folder=Path(cls.tmp.name);cls.outputs={}
        for name in ('left','right','boundary','specialized'):
            out=cls.folder/name;source=ROOT/f'experiments/cell_spine/fixtures/input_spine_{name}.egg'
            r=subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',source,'--output',out,'--skip-build'],
                             cwd=ROOT,capture_output=True,text=True)
            if r.returncode:raise AssertionError(r.stdout+r.stderr)
            cls.outputs[name]=out
    @classmethod
    def tearDownClass(cls):cls.tmp.cleanup()

    def verify(self,fixture,label,compact,extra_reference='',expect_failure=False):
        out=self.outputs[fixture];base=json.loads((out/'work/validation.json').read_text())
        manifest=dict(constructors=base['constructors'],library=base['library'],cases=[dict(name=label,
            reference_path=str(ROOT/f'experiments/cell_spine/fixtures/input_spine_{fixture}.egg'),
            reference=extra_reference,compressed=compact,restore='(run-schedule (saturate Export!))')])
        path=self.folder/f'{label}.json';path.write_text(json.dumps(manifest))
        result=self.folder/f'{label}-result.json'
        r=subprocess.run([ROOT/'target/release/examples/spine_validate',path,result],capture_output=True,text=True)
        if expect_failure:
            self.assertNotEqual(r.returncode,0)
            self.assertIn('full closure mismatch',r.stderr)
            return
        self.assertEqual(r.returncode,0,r.stderr)
        return json.loads(result.read_text())['cases'][0]

    def test_both_associations_construct_the_selected_spine_without_raw_add_copy(self):
        for fixture in ('left','right'):
            out=self.outputs[fixture];report=json.loads((out/'report.json').read_text())
            self.assertEqual(report['input_plan']['status'],'projected')
            library=(out/'library.egg').read_text();body=(out/'runtime.egg').read_text().removeprefix(library)
            self.assertIn('(Add!! ',body)
            self.assertNotIn('(Add! ',body)
            self.assertIn('(Summary-',body)
            data=json.loads((out/'work/input.json').read_text())
            original,_=cellify(data,library)
            before=self.verify(fixture,fixture+'-raw',original.removeprefix(library))
            after=json.loads((out/'validation.json').read_text())['cases'][-1]
            self.assertEqual(after['comparison']['status'],'equivalent')
            self.assertEqual(before['restored_constructor_rows'],after['restored_constructor_rows'])
            self.assertLess(after['native_total_rows_before_export'],before['native_total_rows_before_export'])
            self.assertEqual(after['native_tables_before_export'].get('Add!',0),0)
            # The objective is a separate initializer choice for fixed ABI.
            self.assertIn('fixed',report['input_plan']['scope'])

    def test_named_intermediate_used_outside_cell_is_preserved(self):
        out=self.outputs['boundary'];lib=(out/'library.egg').read_text()
        body=(out/'runtime.egg').read_text().removeprefix(lib)
        self.assertIn('(let inner (Add! ',body)
        self.assertIn('(let held_inner (Add! ',body)
        v=json.loads((out/'validation.json').read_text())['cases'][-1]
        self.assertEqual(v['native_tables_before_export'].get('Add!',0),2)
        self.assertEqual(v['comparison']['status'],'equivalent')

    def test_late_external_fact_can_still_read_the_intermediate(self):
        out=self.outputs['boundary'];lib=(out/'library.egg').read_text()
        body=(out/'runtime.egg').read_text().removeprefix(lib)
        body+='\n(Use! held_inner)\n'+RUNTIME_SCHEDULE
        self.verify('boundary','late-use',body,
            '(Use held_inner)\n(run-schedule (saturate algebra))\n(check (Seen (Leaf "d") (Leaf "e")))')

    def test_union_enables_a_late_boundary_match(self):
        out=self.outputs['boundary'];lib=(out/'library.egg').read_text()
        body=(out/'runtime.egg').read_text().removeprefix(lib)
        body+='\n(let fresh (Leaf! "late")) (Use! fresh)\n'+RUNTIME_SCHEDULE
        body+='\n(union fresh held_inner)\n'+RUNTIME_SCHEDULE
        self.verify('boundary','late-union',body,
            '(let fresh (Leaf "late")) (Use fresh)\n(run-schedule (saturate algebra))\n'
            '(union fresh held_inner)\n(run-schedule (saturate algebra))\n(check (Seen (Leaf "d") (Leaf "e")))')

    def test_specialized_binding_cannot_be_erased_by_generic_node_coverage(self):
        out=self.outputs['specialized'];lib=(out/'library.egg').read_text()
        body=(out/'runtime.egg').read_text().removeprefix(lib)
        report=json.loads((out/'report.json').read_text())
        self.assertGreater(report['input_plan']['semantic_input_obligations'],0)
        self.assertIn('(Add! ',body)
        self.verify('specialized','repeated-binding',body)
        # The generic Cell has the same constructors, but does not establish
        # the additional Repeated(x) effect. Hiding those raw rows loses it.
        self.verify('specialized','unsafe-shape-only',body.replace('(Add! ', '(Add!! '),expect_failure=True)

    def test_initial_union_uses_order_preserving_input_path(self):
        data=dict(actions=[dict(action=dict(kind='union',args=[]))])
        initial,report=project(data,[],{}, {},{},self.folder)
        self.assertIsNone(initial)
        self.assertEqual(report['status'],'raw_ingress')


if __name__=='__main__':unittest.main()

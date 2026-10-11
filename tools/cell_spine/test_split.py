"""Source normalization tests on the actual egglog parser and runtime."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from .split import ROOT, native_ast, plan, publish


class SplitScopes(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=ROOT/'out', prefix='split-test-')
        self.folder = Path(self.temp.name)
        self.addCleanup(self.temp.cleanup)

    def split(self, text):
        source = self.folder/'source.egg'
        source.write_text(text)
        report = plan(source, native_ast(source, self.folder))
        return source, report, publish(report)

    def execute(self, path):
        subprocess.run([ROOT/'target/release/examples/native_math_baseline', path],
                       check=True, cwd=ROOT, capture_output=True, text=True)

    def test_siblings_keep_shared_setup_and_rollback(self):
        source, _, report = self.split('''
(datatype E (A) (B) (C))
(ruleset a)
(ruleset b)
(rewrite (A) (B) :ruleset a)
(rewrite (A) (C) :ruleset b)
(function f (i64) i64 :merge (max old new))
(set (f 0) 5)
(let x (A))
(push)
  (set (f 0) 7)
  (run a 1)
  (check (= x (B)) (= (f 0) 7))
  (fail (check (= x (C))))
(pop)
(check (= (f 0) 5))
(push)
  (run b 1)
  (check (= x (C)) (= (f 0) 5))
  (fail (check (= x (B))))
(pop)
''')
        self.execute(source)
        self.assertEqual([f['ruleset'] for f in report['files']], ['a', 'b'])
        for f in report['files']:
            self.execute(f['path'])
            self.assertEqual(Path(f['path']).parent, source.parent)
            text = Path(f['path']).read_text()
            self.assertNotIn('(push)', text)
            other = 'b' if f['ruleset'] == 'a' else 'a'
            self.assertNotIn(':ruleset '+other, text)

    def test_nested_prefix_runs_are_not_discarded(self):
        _, _, report = self.split('''
(datatype E (A) (B) (C))
(ruleset a) (ruleset b)
(rewrite (A) (B) :ruleset a)
(rewrite (B) (C) :ruleset b)
(A)
(push)
  (run a 1)
  (push)
    (run a 1)
    (check (= (A) (B)))
  (pop)
  (push)
    (run b 1)
    (check (= (A) (C)))
  (pop)
(pop)
''')
        self.assertEqual(len(report['files']), 2)
        self.assertEqual(report['skipped'][0]['rulesets'], ['a', 'b'])
        self.assertEqual(len(report['files'][1]['inherited_schedule_lines']), 1)
        for f in report['files']:
            self.execute(f['path'])

    def test_counted_stack_wildcards_and_annotations_roundtrip(self):
        _, _, report = self.split('''; 中文 (push) 只是注释
(datatype E (A) (Box String))
(ruleset a)
; @egg-viz/v1 {"labels":{"bindings":{"x":"value"}}}
(rule ((= x (Box _))) ((union x (A))) :ruleset a)
(push 2)
  (let x (Box "(pop)"))
  (run a 2)
  (check (= x (A)))
(pop 2)
''')
        self.assertEqual(len(report['files']), 1)
        path = Path(report['files'][0]['path'])
        self.assertIn('(Box _)', path.read_text())
        self.assertIn('@egg-viz/v1', path.read_text())
        self.assertNotIn('@_', path.read_text())
        self.execute(path)

    def test_root_program_keeps_finite_until_schedule_and_checks(self):
        _, _, report = self.split('''
(datatype E (A) (B))
(ruleset a) (ruleset unused)
(rewrite (A) (B) :ruleset a)
(rewrite (B) (A) :ruleset unused)
(A)
(run-schedule (repeat 3 (run a :until (= (A) (B)))))
(check (= (A) (B)))
''')
        self.assertEqual(report['files'][0]['scope'], 0)
        self.assertIn('(repeat 3 (run a :until', Path(report['files'][0]['path']).read_text())
        self.execute(report['files'][0]['path'])

    def test_reruns_reuse_exact_outputs_and_refuse_edited_files(self):
        _, plan_report, first = self.split('(datatype E (A))\n(rule ((A)) ((A)))\n(A)\n(run 1)\n')
        self.assertEqual(publish(plan_report), first)
        output = Path(first['files'][0]['path'])
        output.write_text('; user edit\n')
        with self.assertRaisesRegex(ValueError, 'different content'):
            publish(plan_report)
        self.assertEqual(output.read_text(), '; user edit\n')

    def test_bad_stack_or_opaque_command_writes_nothing(self):
        for text in ['(pop)', '(push) (push) (pop)', '(include "other.egg")']:
            source = self.folder/'source.egg'
            source.write_text(text)
            with self.assertRaises(ValueError):
                plan(source, native_ast(source, self.folder))
            self.assertFalse(list(self.folder.glob('*.split*')))

    def test_cli_places_outputs_beside_selected_symlink_and_dry_run_writes_nothing(self):
        original = self.folder/'original.egg'
        original.write_text('(datatype E (A))\n(rule ((A)) ((A)))\n(A)\n(run 1)\n')
        visible = self.folder/'visible'
        visible.mkdir()
        selected = visible/'linked.egg'
        selected.symlink_to(original)
        cmd = [sys.executable, '-m', 'tools.cell_spine.split', '--source', str(selected), '--skip-build']
        subprocess.run([*cmd, '--dry-run'], check=True, cwd=ROOT, capture_output=True)
        self.assertFalse(list(visible.glob('*.split*')))
        subprocess.run(cmd, check=True, cwd=ROOT, capture_output=True)
        self.assertTrue((visible/'linked.split-000-default.egg').is_file())
        self.assertFalse((self.folder/'original.split-000-default.egg').exists())


if __name__ == '__main__':
    unittest.main()

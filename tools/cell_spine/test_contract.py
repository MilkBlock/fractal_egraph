"""Cell compilation must never publish a debug archive as a successful Bake."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from .compiler import ROOT


class BakeContract(unittest.TestCase):
    def test_no_ac_specific_compiler_is_available(self):
        self.assertIsNone(importlib.util.find_spec('tools.cell_spine.ac'))
        self.assertNotIn('compile_ac', (ROOT/'tools/cell_spine/compiler.py').read_text())

    def test_unsupported_cell_input_does_not_fall_back_to_an_archive(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='bake-contract-') as temp:
            for mode in ('auto','symbolic'):
                out=Path(temp)/mode
                r=subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',
                    ROOT/'experiments/cell_spine/fixtures/stages/multi_rulesets.egg',
                    '--output',out,'--mode',mode,'--skip-build'],cwd=ROOT,capture_output=True,text=True)
                self.assertNotEqual(r.returncode,0)
                self.assertIn('No snapshot fallback',r.stderr)
                for name in ['runtime.egg','library.egg','restore.egg','snapshot.egg','report.json']:
                    self.assertFalse((out/name).exists(),name)

    def test_explicit_archive_has_distinct_names_and_honest_accounting(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='archive-contract-') as temp:
            out=Path(temp)/'out'
            subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',
                ROOT/'experiments/cell_spine/fixtures/stages/finite_updates.egg',
                '--output',out,'--mode','stages','--skip-build'],cwd=ROOT,check=True,capture_output=True)
            r=json.loads((out/'report.json').read_text())
            self.assertEqual(r['status'],'archived_stages')
            self.assertFalse(r['cellified'])
            self.assertEqual(r['compressed'],r['archive_bytes']<r['expanded_recipe_bytes'])
            for name in ['runtime.egg','library.egg','restore.egg']:
                self.assertFalse((out/name).exists(),name)
            for name in ['snapshot.egg','snapshot-restore.egg','snapshot-dictionary.egg','snapshot-library.json']:
                self.assertTrue((out/name).is_file(),name)


if __name__=='__main__':unittest.main()

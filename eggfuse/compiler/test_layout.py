"""Repository entry points must work without legacy forwarding paths."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class LayoutTests(unittest.TestCase):
    def test_direct_harnesses_start_outside_repository(self):
        env = {k: v for k, v in os.environ.items() if k != 'PYTHONPATH'}
        with tempfile.TemporaryDirectory() as cwd:
            for script in ('experiments/device_backend/run.py',
                           'experiments/device_backend/run_generic_cells.py',
                           'experiments/device_backend/test_generic_boundaries.py',
                           'experiments/device_backend/test_scheduled_matrix.py',
                           'experiments/unified_compiler/check.py',
                           'experiments/scheduled_cell_patches/check_library.py'):
                with self.subTest(script=script):
                    result = subprocess.run([sys.executable, str(ROOT / script), '--help'],
                                            cwd=cwd, env=env, text=True, capture_output=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn('usage:', result.stdout)

    def test_rebuilt_worker_keeps_native_provenance_only(self):
        spec = importlib.util.spec_from_file_location('device_build', ROOT / 'experiments/device_backend/build.py')
        build = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(build)
        native = '/saved/egglog/src/device_program.rs'
        refreshed = build.refresh_runtime_provenance({
            native: 'original-native-digest',
            '/saved/experiments/device_backend/worker.swift': 'stale-worker',
            '/saved/tools/eggfuse/pipeline.py': 'stale-compiler',
        })
        self.assertEqual(refreshed[native], 'original-native-digest')
        workers = [p for p in refreshed if p.endswith('/worker.swift')]
        self.assertEqual(workers, [str(ROOT / 'engine/metal/worker.swift')])
        self.assertNotIn('/saved/tools/eggfuse/pipeline.py', refreshed)
        self.assertIn(str(ROOT / 'eggfuse/compiler/targets/metal/compiler.py'), refreshed)
        for path in build.runtime_sources():
            self.assertTrue(path.is_file(), path)
            self.assertFalse(path.is_symlink(), path)


if __name__ == '__main__':
    unittest.main()

"""out/debugger retention regression.

Every web run writes a fresh full snapshot tree, and a single round JSON can reach tens of
MB. Nothing removed them, so the directory reached 14 GB and a run failed with ENOSPC.
"""
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import server

RUN = '%032x'


class PruneRuns(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def make_run(self, name, mtime):
        folder = self.root / name
        (folder / 'rounds').mkdir(parents=True)
        (folder / 'rounds' / 'round-0001.json').write_text('{}')
        os.utime(folder, (mtime, mtime))
        return folder

    def names(self):
        return sorted(p.name for p in self.root.iterdir())

    def test_keeps_only_the_newest_runs(self):
        with patch.object(server, 'RUN_ROOT', self.root):
            ids = [RUN % i for i in range(5)]
            for i, name in enumerate(ids):
                self.make_run(name, 1000.0 + i)
            server.prune_runs(keep=3)
        self.assertEqual(self.names(), sorted(ids[2:]))

    def test_spares_entries_that_are_not_run_ids(self):
        with patch.object(server, 'RUN_ROOT', self.root):
            self.make_run(RUN % 0, 1000.0)
            (self.root / 'not-a-run').mkdir()
            (self.root / 'afile').write_text('x')
            server.prune_runs(keep=1)
        self.assertEqual(self.names(), sorted([RUN % 0, 'afile', 'not-a-run']))

    def test_is_idempotent(self):
        with patch.object(server, 'RUN_ROOT', self.root):
            for i in range(4):
                self.make_run(RUN % i, 1000.0 + i)
            server.prune_runs(keep=2)
            first = self.names()
            server.prune_runs(keep=2)
            self.assertEqual(self.names(), first)
        self.assertEqual(first, sorted([RUN % 2, RUN % 3]))

    def test_missing_root_is_not_an_error(self):
        with patch.object(server, 'RUN_ROOT', self.root / 'absent'):
            server.prune_runs(keep=1)

    def test_limit_is_at_least_one(self):
        self.assertGreaterEqual(server.RUN_LIMIT, 1)


if __name__ == '__main__':
    unittest.main(verbosity=2)

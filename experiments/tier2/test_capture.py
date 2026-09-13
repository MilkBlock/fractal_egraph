import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from run import options
from capture import capture_or_reuse

class CaptureTests(unittest.TestCase):
    def test_eleven_rounds_and_explicit_reuse(self):
        a=options(['--recapture-tier0','--rounds','11','--output','out/math11'])
        self.assertTrue(a.recapture_tier0);self.assertEqual(a.rounds,11)
        self.assertFalse(options(['--reuse-tier0']).recapture_tier0)
    def test_source_is_preserved_and_missing_input_does_not_create_output(self):
        a=options(['--recapture-tier0','--source','a directory/custom.egg','--rounds','11'])
        self.assertEqual(a.source,Path('a directory/custom.egg'))
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);out=root/'new-run'
            with self.assertRaises(FileNotFoundError):capture_or_reuse(root,root/'driver',out,2,True,Path('missing.egg'))
            self.assertFalse(out.exists())
    def test_rounds_cannot_silently_apply_to_a_saved_snapshot(self):
        for args in [['--rounds','11'],['--source','file.egg'],['--recapture-tier0','--rounds','0'],['--recapture-tier0','--reuse-tier0']]:
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):options(args)
    def test_incomplete_run_cannot_fallback(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);(root/'run.json').write_text(json.dumps({'status':'failed'}))
            with patch('capture.subprocess.run') as execute:
                with self.assertRaises(ValueError):capture_or_reuse(root,root/'driver',root,11,False)
                execute.assert_not_called()
    def test_capture_cannot_replace_existing_directory(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            with self.assertRaises(FileExistsError):capture_or_reuse(root,root/'driver',root,11,True)
    def test_failed_reuse_is_marked_failed(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);marker=root/'run.json';marker.write_text(json.dumps({'status':'complete','executed_rounds':11}))
            with patch('capture.subprocess.run',side_effect=RuntimeError('test failure')):
                with self.assertRaises(RuntimeError):capture_or_reuse(root,root/'driver',root,11,False)
            self.assertEqual(json.loads(marker.read_text())['status'],'failed')
            self.assertEqual(json.loads(marker.read_text())['executed_rounds'],11)
if __name__=='__main__':unittest.main()

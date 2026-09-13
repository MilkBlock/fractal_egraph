import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from capture import profile_pipe,command_pipe
ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('bridge',ROOT/'research/math_bridge.py')
bridge=importlib.util.module_from_spec(spec);spec.loader.exec_module(bridge)
class StreamTests(unittest.TestCase):
    def test_bridge_commands_equal_previous_file_pipeline(self):
        raw=json.loads((ROOT/'tests/fixtures/bridge_profile.json').read_text())
        lines,manifest=bridge.build(raw,streaming=True)
        self.assertNotIsInstance(lines,str)
        indexed='\n'.join(lines)+'\n'
        self.assertIn('(function ImportedInstance (i64) Instance :no-merge)',indexed)
        self.assertNotIn('(let $i0',indexed)
        self.assertEqual(bridge.build(raw)[0],(ROOT/'tests/fixtures/bridge_program.egg').read_text())
        self.assertEqual(manifest['audit']['imported_events'],2)
        self.assertTrue(all('input_layout' in r for r in manifest['records']))
    def test_profile_protocol_and_no_files(self):
        with tempfile.TemporaryDirectory() as d:
            frames=[['array','events'],['item','events',{'id':7}],['value','executed_rounds',11],['end']]
            code='import sys;sys.stdout.write('+repr(''.join(json.dumps(x)+'\n' for x in frames))+')'
            result=profile_pipe([sys.executable,'-c',code],Path(d))
            self.assertEqual(result,{'events':[{'id':7}],'executed_rounds':11})
            self.assertEqual(list(Path(d).iterdir()),[])
    def test_truncated_or_failed_capture_cannot_succeed(self):
        with tempfile.TemporaryDirectory() as d:
            with self.assertRaises(ValueError):profile_pipe([sys.executable,'-c','print(\'["array","events"]\')'],Path(d))
            with self.assertRaises(subprocess.CalledProcessError):profile_pipe([sys.executable,'-c','import sys;print(\'["end"]\');sys.exit(3)'],Path(d))
    def test_generated_command_pipe(self):
        with tempfile.TemporaryDirectory() as d:
            code='import sys,json;print(json.dumps(list(sys.stdin)))'
            self.assertEqual(command_pipe([sys.executable,'-c',code],iter(['one','two']),Path(d)),['one\n','two\n'])
            self.assertEqual(list(Path(d).iterdir()),[])
if __name__=='__main__':unittest.main()

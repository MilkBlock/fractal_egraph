"""Fresh-process, source-only contract tests; no standalone mock executor."""
import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from .compiler import ROOT
from .structural_keys import compile_source as keys
from .source_pipeline import run as markers
from . import test_selected_source as selected_tests
sys.path.insert(0,str(ROOT/'skills/cell-assisted-egglog-bench/scripts'))
from program_modes import compare

HEADER='; cell-runtime/v1 '

class SourcePipeline(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='source-only-')
        cls.root=Path(cls.temp.name)
        cls.source=cls.root/'source.egg'
        cls.source.write_text((ROOT/'experiments/parameterized_source_markers/counter.egg').read_text())
        keys(cls.source,cls.root/'sk')
        cls.sk=cls.root/'source_sk.egg';shutil.copyfile(cls.root/'sk/source_sk.egg',cls.sk)
        cls.selection=markers(cls.sk,cls.root/'mk')
        cls.final=cls.root/'standalone.egg';shutil.copyfile(cls.root/'mk/source_sk_mk.egg',cls.final)
        # Prove there are no execution dependencies on compiler artifacts.
        shutil.rmtree(cls.root/'sk');shutil.rmtree(cls.root/'mk')
        cls.original_text=cls.source.read_text();cls.source.unlink();cls.sk.unlink()
        cls.seq=0
    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()

    def invoke(self,source,backend='cpu',extra=(),env_extra=None,ok=True):
        type(self).seq+=1;out=self.root/f'run-{self.seq}'
        cmd=[str(ROOT/'target/analyzer/release/cell-assisted-egglog'),'--mode','program',
            '--backend',backend,'--source',str(source),'--output',str(out),*extra]
        env={k:v for k,v in os.environ.items() if not k.startswith(('CELL_','EGGLOG_'))}
        env.update(env_extra or {})
        p=subprocess.run(cmd,env=env,capture_output=True,text=True)
        if ok:self.assertEqual(p.returncode,0,p.stdout+p.stderr)
        else:self.assertNotEqual(p.returncode,0)
        return out,p

    def test_final_source_alone_runs_and_reports_actual_contexts(self):
        native,_=self.invoke(self.final,'native')
        assisted,_=self.invoke(self.final)
        g=lambda d:json.loads((d/'graph.json').read_text())
        self.assertEqual(compare(g(native),g(assisted))['status'],'equal')
        d=json.loads((assisted/'cells.json').read_text())
        self.assertEqual(len(d['declared_queries']),self.selection['retained_query_contexts'])
        self.assertGreater(len(d['declared_queries']),0)
        self.assertIsNone(d['runtime']['joint'])
        self.assertEqual(d['runtime']['scalar_cells'],[])
        self.assertGreater(sum(p['emitted_bindings'] for p in d['runtime']['programs']),0)
        # Counter contains six interleaved Runs/checks, updates and a union.
        r=json.loads((assisted/'report.json').read_text())
        self.assertGreaterEqual(r['runtime']['runtime']['ruleset_runs'],6)
        self.assertFalse(r['source_rewritten_by_runner'])
        original=self.root/'raw-reference.egg';original.write_text(self.original_text)
        raw,_=self.invoke(original,'native')
        self.assertEqual(compare(g(raw),g(assisted),set(g(raw)['tables']))['status'],'equal')

    def test_gpu_accepts_the_same_inline_plans(self):
        worker=os.environ.get('CELL_MATRIX_WORKER')
        if not worker:self.skipTest('set CELL_MATRIX_WORKER for actual GPU execution')
        native,_=self.invoke(self.final,'native')
        gpu,_=self.invoke(self.final,'gpu',['--gpu-worker',worker,'--query-audit','on'])
        a=json.loads((native/'graph.json').read_text());b=json.loads((gpu/'graph.json').read_text())
        self.assertEqual(compare(a,b)['status'],'equal')
        self.assertTrue(json.loads((gpu/'cells.json').read_text())['assistance_enabled'])

    def test_source_or_query_tampering_is_rejected(self):
        line,body=self.final.read_text().split('\n',1);d=json.loads(line[len(HEADER):])
        bad=self.root/'bad.egg';bad.write_text(line+'\n'+body+'\n(N 999)\n')
        _,p=self.invoke(bad,ok=False);self.assertIn('does not match',p.stderr)
        d['queries'][0]['plan']['seed']=999
        bad.write_text(HEADER+json.dumps(d)+'\n'+body)
        _,p=self.invoke(bad,ok=False);self.assertIn('anchor mismatch',p.stderr)
        bad.write_text(line+'\n'+line+'\n'+body)
        _,p=self.invoke(bad,ok=False);self.assertIn('duplicate cell-runtime',p.stderr)

    def test_external_overrides_and_legacy_setup_are_rejected(self):
        _,p=self.invoke(self.final,extra=['--boundary-programs','missing.json'],ok=False)
        self.assertIn('unsupported program-mode option',p.stderr)
        _,p=self.invoke(self.final,extra=['--setup-policy','legacy-stable'],ok=False)
        self.assertIn('unsupported program-mode option',p.stderr)
        _,p=self.invoke(self.final,env_extra={'CELL_CERTIFICATE_LIBRARY':'missing.egg'},ok=False)
        self.assertIn('not an input',p.stderr)

    def test_source_marker_tables_are_recorded_without_external_plans(self):
        folder=self.root/'explicit';folder.mkdir()
        source=folder/'source.egg';source.write_text(self.original_text)
        helper=selected_tests.ParameterizedSourceMarkers()
        target,report=helper.compile(source,folder,'State')
        self.assertGreater(report['publish_actions'],0)
        isolated=self.root/'explicit-only.egg';shutil.copyfile(target,isolated)
        shutil.rmtree(folder)
        reference,_=self.invoke(isolated,'native');actual,_=self.invoke(isolated)
        self.assertEqual(compare(json.loads((reference/'graph.json').read_text()),json.loads((actual/'graph.json').read_text()))['status'],'equal')
        markers_=json.loads((actual/'cells.json').read_text())['markers']
        self.assertTrue(markers_);self.assertTrue(any(m['rows']>0 for m in markers_))

    def test_renaming_rules_and_sorts_does_not_enable_different_services(self):
        folder=self.root/'renamed';folder.mkdir()
        source=folder/'source.egg'
        source.write_text(self.original_text.replace('State','Whatever').replace('Port','Outlet')
            .replace('advance','query-one').replace('observe','query-two').replace('(ruleset s)','(ruleset terms)')
            .replace(':ruleset s',':ruleset terms').replace('(run s ', '(run terms '))
        keys(source,folder/'sk')
        result=markers(folder/'sk/source_sk.egg',folder/'mk')
        self.assertEqual(result['selected_queries'],self.selection['selected_queries'])
        out,_=self.invoke(folder/'mk/source_sk_mk.egg')
        d=json.loads((out/'cells.json').read_text())
        self.assertIsNone(d['runtime']['joint']);self.assertEqual(d['runtime']['scalar_cells'],[])
        self.assertEqual({q['rule'] for q in d['declared_queries']},{'query-one','query-two'})

"""Portable source-only closure declarations, including rejection and Run order."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from .compiler import ROOT

sys.path.insert(0,str(ROOT/'skills/cell-assisted-egglog-bench/scripts'))
from program_modes import compare


class DeclaredClosures(unittest.TestCase):
    def test_compilation_rejects_external_input_before_reading_it(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out') as temp:
            root=Path(temp);source=root/'input.egg'
            source.write_text('(include "this-file-must-not-be-opened.egg")')
            p=subprocess.run([ROOT/'target/analyzer/release/examples/scheduled_spine',source,root/'out.json','--closures'],capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0)
            self.assertIn('self-contained',p.stderr)

    def seal(self,root,rename=False):
        source=root/'input.egg'
        text=(ROOT/'experiments/parameterized_cell/terms.egg').read_text()+'\n'+(ROOT/'experiments/parameterized_cell/competing_updates.egg').read_text()
        if rename:
            for a,b in [('terms','phase'),('ExtractedExpr','Answer'),('TCPair','Weighted'),('Smaller','Choose'),('PotentialExtractedExpr','Offer'),('NoTerm','Missing')]:text=text.replace(a,b)
        source.write_text(text)
        tool=ROOT/'target/analyzer/release/examples/scheduled_spine'
        inventory=root/'inventory.json'
        subprocess.run([tool,source,inventory,'--all-schedules','--closures'],check=True,capture_output=True)
        d=json.loads(inventory.read_text())
        layout=root/'layout.json';layout.write_text(json.dumps(dict(schema='cell-boundary-programs/v2',setup=d['preserved_commands'][:d['setup_commands']],rules=[])))
        final=root/'final.egg'
        subprocess.run([tool,source,final,'--closures','--seal',layout],check=True,capture_output=True)
        source.unlink();layout.unlink();inventory.unlink()
        return final

    def invoke(self,source,out,extra=(),ok=True):
        env={k:v for k,v in os.environ.items() if not k.startswith(('CELL_','EGGLOG_'))}
        p=subprocess.run([ROOT/'target/analyzer/release/cell-assisted-egglog','--mode','program','--backend','cpu','--source',source,'--output',out,*extra],env=env,capture_output=True,text=True)
        self.assertEqual(p.returncode==0,ok,p.stdout+p.stderr)
        return p

    def test_source_only_updates_aliases_and_renamed_roles(self):
        for rename in (False,True):
            with self.subTest(rename=rename),tempfile.TemporaryDirectory(dir=ROOT/'out') as temp:
                root=Path(temp);source=self.seal(root,rename)
                self.invoke(source,root/'on')
                self.invoke(source,root/'off',['--cell-assistance','off'])
                a=json.loads((root/'on/graph.json').read_text());b=json.loads((root/'off/graph.json').read_text())
                self.assertEqual(compare(b,a,set(b['tables']))['status'],'equal')
                cells=json.loads((root/'on/cells.json').read_text())
                self.assertTrue(cells['declared_closures']['programs'])
                self.assertEqual(cells['runtime']['stats']['program_compilations'],0)
                self.assertGreater(cells['runtime']['joint']['stats']['boundaries'],0)

    def test_tampered_closure_wiring_cannot_authorize_execution(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out') as temp:
            root=Path(temp);source=self.seal(root)
            header,body=source.read_text().split('\n',1);prefix='; cell-runtime/v1 '
            doc=json.loads(header[len(prefix):]);doc['closures']['programs'][0]['seed']=999
            source.write_text(prefix+json.dumps(doc)+'\n'+body)
            p=self.invoke(source,root/'bad',ok=False)
            self.assertIn('invalid transfer seed',p.stderr)

    def test_ordinary_run_does_not_execute_a_whole_closure(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out') as temp:
            root=Path(temp);source=self.seal(root)
            header,body=source.read_text().split('\n',1)
            # Recompile a source that explicitly asks for only one producer Run.
            body=body[:body.index('(run-schedule')]+'''(run terms 1)
(check (PotentialExtractedExpr (Bop (Add) (V 0) (V 1)) (TCPair (TermBop (Add) (TermLeaf 0) (TermLeaf 1)) 12)))
(fail (check (= (ExtractedExpr (Bop (Add) (V 0) (V 1))) (TCPair (TermBop (Add) (TermLeaf 0) (TermLeaf 1)) 12))))
'''
            raw=root/'finite.egg';raw.write_text(body)
            tool=ROOT/'target/analyzer/release/examples/scheduled_spine';inv=root/'inv.json'
            subprocess.run([tool,raw,inv,'--closures'],check=True,capture_output=True)
            d=json.loads(inv.read_text());layout=root/'layout.json'
            layout.write_text(json.dumps(dict(schema='cell-boundary-programs/v2',setup=d['preserved_commands'][:d['setup_commands']],rules=[])))
            final=root/'finite-sealed.egg';subprocess.run([tool,raw,final,'--closures','--seal',layout],check=True,capture_output=True)
            self.invoke(final,root/'run')

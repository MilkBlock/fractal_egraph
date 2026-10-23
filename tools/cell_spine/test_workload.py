import copy
import gzip
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from types import SimpleNamespace
from . import workload,scheduled,solver
from .compiler import ROOT


class RuntimeWorkload(unittest.TestCase):
    def test_runtime_pricing_rejects_mixed_static_profiles_before_execution(self):
        a=SimpleNamespace(output=Path('unused'),source=Path('unused'),runtime_workload=Path('runtime.json'),static_cost_experiment=False,query_profile=None,table_profile=Path('static.json'))
        with self.assertRaisesRegex(ValueError,'must not be mixed'):
            scheduled.compile_program(a,{},ROOT,(1,1,1))

    def fixture(self,folder):
        source=ROOT/'experiments/parameterized_source_markers/counter.egg';raw=folder/'profile.jsonl';packed=folder/'profile.jsonl.gz'
        subprocess.run([ROOT/'target/analyzer/release/examples/planner_profile',source,raw],check=True,capture_output=True)
        with gzip.open(packed,'wb') as f:f.write(raw.read_bytes())
        manifest=folder/'manifest.json'
        manifest.write_text(json.dumps(dict(schema='marker-workload-corpus/v1',complete=True,
            identity=dict(workloads=[dict(source=str(source),sha256=workload.digest(source))]),
            cases=[dict(source=str(source),source_sha256=workload.digest(source),status='complete',passes=[dict(name='pass-1',input=str(source),input_sha256=workload.digest(source),profile=str(packed),profile_sha256=workload.digest(packed),status='complete')])])) )
        return manifest,json.loads(raw.read_text().splitlines()[0])['header']

    def test_actual_rounds_price_plans_and_ignore_initial_table_sizes(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out') as temp:
            folder=Path(temp);manifest,header=self.fixture(folder);cost=workload.collect(manifest)
            self.assertGreater(cost['cases'][0]['rounds'],3)
            self.assertTrue(all(p['requests']>0 for p in cost['plans'].values()))
            q=header['queries'][0];opts=workload.options(q,cost)
            self.assertTrue(opts);self.assertTrue(all(o['cost_evidence']['kind'].startswith('measured') for o in opts))
            solver.select('scip')
            a=scheduled.select(header['queries'],{},folder,(1,1,1),runtime_workload=cost)
            b=scheduled.select(header['queries'],{'State':10**30,'Port':0},folder,(1,1,1),runtime_workload=cost)
            self.assertEqual(a['objective'],b['objective'])
            self.assertEqual([x['plan'] for x in a['selected']],[x['plan'] for x in b['selected']])
            unknown=copy.deepcopy(q);unknown['shape']['guards'].append([0,[{'Slot':0},{'Literal':'999'}]])
            self.assertEqual(workload.options(unknown,cost),[])
            self.assertTrue(any(p['live_prefixes'][0]>1 for p in cost['plans'].values()))

    def test_incomplete_or_modified_evidence_never_becomes_zero_cost(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out') as temp:
            folder=Path(temp);manifest,_=self.fixture(folder);m=json.loads(manifest.read_text())
            m['complete']=False;manifest.write_text(json.dumps(m))
            with self.assertRaisesRegex(ValueError,'complete'):workload.collect(manifest)
            m['complete']=True;m['identity']['workloads'].append(dict(source='missing',sha256='x'));manifest.write_text(json.dumps(m))
            with self.assertRaisesRegex(ValueError,'coverage'):workload.collect(manifest)
            m['identity']['workloads'].pop();manifest.write_text(json.dumps(m))
            Path(m['cases'][0]['passes'][0]['profile']).write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError,'provenance'):workload.collect(manifest)

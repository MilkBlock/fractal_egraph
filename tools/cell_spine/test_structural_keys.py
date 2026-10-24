"""Explicit shared DAG source; SK is the oracle for marker equivalence."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from .compiler import ROOT
from .structural_keys import compile_source,HEADER,HELPER
from .pass3_markers import compile_source as mark
sys.path.insert(0,str(ROOT/'skills/cell-assisted-egglog-bench/scripts'))
from program_modes import compare


class SourceDag(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='dag-key-test-')
        self.addCleanup(self.tmp.cleanup);self.out=Path(self.tmp.name)

    def execute(self,source,dest):
        p=subprocess.run([ROOT/'target/analyzer/release/cell-assisted-egglog','--mode','program','--backend','native','--source',source,'--output',dest],capture_output=True,text=True)
        self.assertEqual(p.returncode,0,p.stderr)
        report=json.loads((dest/'report.json').read_text())
        self.assertTrue(report['snapshot_complete'])
        self.assertEqual(report['setup']['runtime_extension']['name'],'cell-structural-key-runtime/v1')
        return json.loads((dest/'graph.json').read_text())

    def test_sk_is_marker_equivalence_oracle(self):
        for fixture in ['automatic','markers']:
            with self.subTest(fixture=fixture):
                folder=self.out/fixture;folder.mkdir()
                report=compile_source(ROOT/f'experiments/standalone_structural_keys/{fixture}.egg',folder/'sk')
                sk=folder/'sk/source_sk.egg';a=self.execute(sk,folder/'native-sk')
                m=mark(sk,ROOT/'experiments/analyze_library_credit/library',folder/'mk',ROOT/'target/analyzer/release/examples/lhs_ast')
                mk=Path(m['variants']['published']['path']);b=self.execute(mk,folder/'native-mk')
                self.assertEqual(compare(a,b,set(a['tables']))['status'],'equal')
                self.assertEqual(sk.read_text().splitlines()[0],mk.read_text().splitlines()[0])
                self.assertNotIn('BigRat',sk.read_text());self.assertNotIn('BigInt',sk.read_text())
                self.assertTrue(report['original_choices_preserved'])
                if fixture=='markers':
                    self.assertEqual(m['modified_consumers'],3)
                    self.assertTrue(any(row[0]=='CellSpineView0' for row in b['rows']))

    def test_repeated_subtree_stays_a_shared_dag_in_runtime_and_snapshot(self):
        depth=30
        source=self.out/'source.egg'
        lines=['(datatype Expr (N i64) (Pair Expr Expr))',
            '(function Expr-size (Expr) i64 :merge (max old new))',
            '(function to-hoist (Expr Expr) Expr :merge new)','(ruleset r)','(let x0 (N 1))']
        lines += [f'(let x{i} (Pair x{i-1} x{i-1}))' for i in range(1,depth+1)]
        lines += ['(run r 1)'];source.write_text('\n'.join(lines))
        compile_source(source,self.out/'sk');g=self.execute(self.out/'sk/source_sk.egg',self.out/'native')
        nodes=g['structural_key_dag'];pairs=[n for n in nodes if n['label']=='Expr:Pair']
        self.assertEqual(len(pairs),depth)
        self.assertTrue(all(n['children'][0]==n['children'][1] for n in pairs))
        self.assertLessEqual(len(nodes),3*depth+10)  # expanded tree would exceed a billion nodes

    def test_dynamic_literals_are_supported_by_the_native_extension(self):
        source=self.out/'source.egg'
        source.write_text('''(datatype Expr (S String))
(function Expr-size (Expr) i64 :merge (min old new))
(function to-hoist (Expr Expr) Expr :merge new)
(relation Candidate (Expr)) (ruleset grow) (ruleset pick)
(rule ((= e (S "a"))) ((Candidate (S (+ "a" "b"))) (set (Expr-size (S (+ "a" "b"))) 2)) :ruleset grow)
(rule ((Candidate x)) ((set (to-hoist (S "a") (S "a")) x)) :ruleset pick)
(S "a") (run grow 1) (run pick 1) (check (= (to-hoist (S "a") (S "a")) (S "ab")))''')
        compile_source(source,self.out/'sk');self.execute(self.out/'sk/source_sk.egg',self.out/'native')

    def test_application_specific_selection_policy_is_removed(self):
        with self.assertRaisesRegex(ValueError,'application-specific'):
            compile_source(ROOT/'experiments/standalone_structural_keys/hoist.egg',self.out/'sk',policy='full')

    def test_no_equivalence_claim_uses_only_hashes_or_local_dag_ids(self):
        def graph(nodes,root):return dict(tables={'k':dict(input=[],output='CellStructuralKey')},rows=[['k',[],['CellStructuralKey',{'key':root}],False]],structural_key_dag=nodes)
        a=graph([dict(anchor=False,label='a',children=[]),dict(anchor=False,label='p',children=[0,0])],1)
        b=graph([dict(anchor=False,label='unused',children=[]),dict(anchor=False,label='a',children=[]),dict(anchor=False,label='p',children=[1,1])],2)
        self.assertEqual(compare(a,b)['status'],'equal')
        b['structural_key_dag'][1]['label']='b'
        self.assertEqual(compare(a,b)['status'],'different')
        b['structural_key_dag'][1]['children']=[1]
        with self.assertRaises(ValueError):compare(a,b)

    def test_key_type_closure_ignores_unrelated_analysis_relations(self):
        source=self.out/'source.egg'
        source.write_text('''(datatype Expr (N i64)) (datatype Unrelated (Ignored i64))
(function Expr-size (Expr) i64 :merge (min old new))
(function to-hoist (Expr Expr) Expr :merge new)
(relation Analysis (Expr Unrelated)) (ruleset r) (Analysis (N 1) (Ignored 2)) (run r 1)''')
        report=compile_source(source,self.out/'sk')
        self.assertEqual(report['key_sorts'],['Expr','Unrelated']);self.assertEqual(report['key_constructors'],2)
        self.execute(self.out/'sk/source_sk.egg',self.out/'native')

    def test_function_names_do_not_enable_a_hidden_selection_policy(self):
        src=self.out/'input.egg';src.write_text('(datatype Expr (N i64)) (function to-hoist (Expr) Expr :merge new) (ruleset r) (N 1) (run r 1)')
        compile_source(src,self.out/'sk')
        self.assertIn('(function to-hoist (Expr) Expr :merge new)',(self.out/'sk/source_sk.egg').read_text())

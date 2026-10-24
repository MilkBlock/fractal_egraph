"""Demand pruning, explicit choice semantics, and real native execution."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from .compiler import ROOT
from .structural_keys import compile_source, HEADER, HELPER
from .key_demand import CHOICE


class KeyDemand(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='key-demand-test-')
        self.addCleanup(self.tmp.cleanup);self.root=Path(self.tmp.name)

    def compile(self,code,name='built'):
        src=self.root/(name+'.egg');src.write_text(code)
        report=compile_source(src,self.root/name)
        return self.root/name/'source_sk.egg',report

    def run_source(self,source,name='run',backend='native'):
        cmd=[str(ROOT/'target/analyzer/release/cell-assisted-egglog'),'--mode','program','--backend',backend,
             '--source',str(source),'--output',str(self.root/name)]
        if backend=='gpu':cmd+=['--gpu-worker',os.environ['CELL_MATRIX_WORKER'],'--query-audit','on']
        p=subprocess.run(cmd,capture_output=True,text=True)
        return p,self.root/name

    def test_no_reader_produces_no_tables_rules_or_maintenance(self):
        src,report=self.compile('(datatype E (N i64)) (function arbitrary (E) E :merge new) (ruleset s) (N 7) (run s 1)')
        self.assertEqual(report['key_sorts'],[]);self.assertEqual(report['key_constructors'],0)
        self.assertEqual(report['native_rulesets'],[]);self.assertTrue(report['original_choices_preserved'])
        self.assertTrue(report['original_body_preserved'])
        code=src.read_text().split('\n',1)[1]
        self.assertNotIn('CellStructuralKey-',code);self.assertNotIn(HELPER,code)
        self.assertIn('(function arbitrary (E) E :merge new)',code)
        self.assertEqual(self.run_source(src)[0].returncode,0)

    def test_candidates_subtrees_only_and_union_of_choice_keys(self):
        src,report=self.compile((ROOT/'experiments/structural_key_demand/choice.egg').read_text())
        self.assertEqual(report['key_sorts'],['E']);self.assertEqual(report['maintenance_before'],['pick'])
        self.assertIn('Unused',report['pruned_key_sorts']);self.assertEqual(report['key_constructors'],2)
        code=src.read_text();self.assertNotIn('cell-key-request',next(l for l in code.splitlines() if l.startswith('(run-schedule') and '(run ordinary)' in l))
        p,out=self.run_source(src);self.assertEqual(p.returncode,0,p.stderr)
        g=json.loads((out/'graph.json').read_text())
        unrelated=next(r[2] for r in g['rows'] if r[0]=='N' and r[1][0][1].get('base')=='999')
        key_rows=[r for r in g['rows'] if r[0]=='CellStructuralKey-E']
        self.assertEqual(len(key_rows),6)
        self.assertFalse(any(r[1]==[unrelated] for r in key_rows))
        if os.environ.get('CELL_MATRIX_WORKER'):
            p,gpu=self.run_source(src,'gpu','gpu');self.assertEqual(p.returncode,0,p.stderr)
            import sys
            sys.path.insert(0,str(ROOT/'skills/cell-assisted-egglog-bench/scripts'))
            from program_modes import compare
            self.assertEqual(compare(g,json.loads((gpu/'graph.json').read_text()))['status'],'equal')

    def test_renaming_does_not_affect_liveness_or_ordering(self):
        code=(ROOT/'experiments/structural_key_demand/choice.egg').read_text()
        for a,b in [('winner','alternative'),('rank','priority'),('Pair','Fork'),('pick','select-set')]:code=code.replace(a,b)
        src,r=self.compile(code);self.assertEqual(r['selected_choices'],['alternative'])
        self.assertEqual(r['maintenance_before'],['select-set'])
        p,_=self.run_source(src);self.assertEqual(p.returncode,0,p.stderr)

    def test_primitive_min_score_handles_i64_extremes_without_negation(self):
        declaration=CHOICE+json.dumps(dict(function='f',score='score',order='min',tie_break='structural'))
        code=declaration+'''\n(datatype E (N i64))
        (function score (E) i64 :merge (max old new)) (function f () E :merge new)
        (set (score (N 1)) -9223372036854775808) (set (score (N 2)) 9223372036854775807)
        (set (f) (N 2)) (set (f) (N 1)) (set (f) (N 1))
        (ruleset s) (run s 1) (check (= (f) (N 1)))'''
        src,_=self.compile(code);p,_=self.run_source(src);self.assertEqual(p.returncode,0,p.stderr)

    def test_missing_key_or_score_fails_before_choice_not_silently_filtered(self):
        for extra in ['', '(subsume (N 1))']:
            decl=CHOICE+json.dumps(dict(function='f',tie_break='structural',**({'score':'score'} if not extra else {})))
            code=decl+'''\n(datatype E (N i64)) (function score (E) i64 :merge (max old new))
            (function f () E :merge new) (relation P (E)) (ruleset s)
            (rule ((P x)) ((set (f) x)) :ruleset s) (P (N 1))\n'''+extra+'\n(run s 1)'
            name='missing-score' if not extra else 'missing-key';src,_=self.compile(code,name)
            p,_=self.run_source(src,name+'-run');self.assertNotEqual(p.returncode,0)
            self.assertIn('unavailable structural key or score',p.stderr)

    def test_live_unsupported_fields_and_fresh_rhs_are_rejected(self):
        decl=CHOICE+json.dumps(dict(function='f',tie_break='structural'))
        sources=[('(sort E) (sort Es (Vec E)) (constructor Pack (Es) E) (function f () E :merge new) (ruleset s) (set (f) (Pack (vec-empty))) (run s 1)','unsupported field'),
            ('(datatype E (N i64)) (function f () E :merge new) (relation P (i64)) (ruleset s) (rule ((P i)) ((set (f) (N i))) :ruleset s) (P 1) (run s 1)','fresh RHS candidate')]
        for i,(source,error) in enumerate(sources):
            with self.assertRaisesRegex(ValueError,error):self.compile(decl+'\n'+source,'invalid'+str(i))

    def test_ranked_candidate_detection_is_syntactic_not_an_implicit_rewrite(self):
        source='''(datatype E (N i64)) (function score (E) i64 :merge (min old new))
        (function best (E) i64 :merge (max old new)) (function f (E) E :merge new)
        (relation Offer (E E)) (ruleset s)
        (rule ((Offer k v) (= (score v) (best k))) ((set (f k) v)) :ruleset s)
        (run s 1)'''
        _,r=self.compile(source)
        self.assertEqual(r['key_demand']['candidates'][0]['ranked_guards'],[dict(score='score',order='max',best='best')])
        self.assertEqual(r['key_sorts'],[])

    def test_combined_sets_and_recursive_type_dependency(self):
        source=CHOICE+json.dumps(dict(function='f',tie_break='structural'))+'''\n
        (sort A) (sort B) (constructor Leaf () A) (constructor AB (B) A) (constructor BA (A) B)
        (function f () A :merge new) (relation P (A)) (ruleset s) (ruleset empty)
        (rule ((P x)) ((set (f) x)) :ruleset s)
        (unstable-combined-ruleset combined s empty) (P (AB (BA (Leaf))))
        (run combined 1) (check (= (f) (AB (BA (Leaf)))))'''
        src,r=self.compile(source);self.assertEqual(r['key_sorts'],['A','B'])
        self.assertEqual(r['maintenance_before'],['combined','s'])
        p,_=self.run_source(src);self.assertEqual(p.returncode,0,p.stderr)

    def test_multiple_selectors_share_sort_keys_without_map_sort_ambiguity(self):
        declarations='\n'.join(CHOICE+json.dumps(dict(function=f,tie_break='structural')) for f in ['f','g'])
        source=declarations+'''\n(datatype E (N i64))
        (sort ExistingMap (Map bool E))
        (function f () E :merge new) (function g () E :merge old)
        (set (f) (N 2)) (set (f) (N 1))
        (set (g) (N 1)) (set (g) (N 2))
        (ruleset s) (run s 1) (check (= (f) (N 1)) (= (g) (N 1)))'''
        src,r=self.compile(source)
        self.assertEqual(r['key_sorts'],['E'])
        self.assertNotIn('map-get',src.read_text())
        p,_=self.run_source(src);self.assertEqual(p.returncode,0,p.stderr)

    def test_candidate_alias_and_function_key_union_in_one_run(self):
        source=CHOICE+json.dumps(dict(function='f',tie_break='structural'))+'''\n
        (datatype E (N i64)) (function f (E) E :merge new)
        (N 100) (set (f (N 10)) (N 1)) (set (f (N 11)) (N 2))
        (ruleset u)
        (rule () ((union (N 100) (N 1)) (union (N 10) (N 11))) :ruleset u)
        (run u 1) (check (= (f (N 10)) (N 1)))'''
        src,_=self.compile(source);p,_=self.run_source(src);self.assertEqual(p.returncode,0,p.stderr)

    def test_marker_stage_keeps_key_helpers_out_of_application_plans(self):
        from .source_pipeline import run
        source=(ROOT/'experiments/structural_key_demand/choice.egg').read_text()
        src,_=self.compile(source)
        run(src,self.root/'markers')
        inventory=json.loads((self.root/'markers/inventory.json').read_text())
        self.assertFalse(any(q['ruleset'].startswith('cell-') for q in inventory['queries']))
        final=self.root/'markers/source_sk_mk.egg'
        p,_=self.run_source(final,'final-native');self.assertEqual(p.returncode,0,p.stderr)

    def test_zero_key_program_still_lowers_surface_rewrites(self):
        src,r=self.compile('''(datatype E (N i64) (Add E E)) (ruleset s)
        (rewrite (Add x (N 0)) x :ruleset s)
        (Add (N 1) (N 0)) (run s 1) (check (= (Add (N 1) (N 0)) (N 1)))''')
        self.assertEqual(r['key_sorts'],[]);self.assertFalse(r['original_body_preserved'])
        p,_=self.run_source(src);self.assertEqual(p.returncode,0,p.stderr)

    def test_dead_selection_and_destructive_scores_do_not_sneak_in(self):
        decl=CHOICE+json.dumps(dict(function='f',tie_break='structural'))
        src,r=self.compile(decl+'''\n(datatype E (N i64)) (function f () E :merge new)
        (ruleset unused) (ruleset active) (relation P (E))
        (rule ((P x)) ((set (f) x)) :ruleset unused) (P (N 1)) (run active 1)''')
        self.assertEqual(r['key_sorts'],[]);self.assertEqual(r['key_demand']['ignored_declarations'],['f'])
        self.assertEqual(self.run_source(src)[0].returncode,0)
        scored=CHOICE+json.dumps(dict(function='g',score='score',tie_break='structural'))
        with self.assertRaisesRegex(ValueError,'deleted/subsumed'):
            self.compile(scored+'''\n(datatype E (N i64)) (function score (E) i64 :merge (min old new))
            (function g () E :merge new) (ruleset s) (set (score (N 1)) 5) (set (g) (N 1))
            (delete (score (N 1))) (run s 1)''','destructive')

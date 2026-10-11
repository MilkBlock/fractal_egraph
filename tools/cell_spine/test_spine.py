import itertools
import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from .compiler import ROOT, load_candidates, parent_wirings, lower_option
from .planner import solve


class Planning(unittest.TestCase):
    def test_shared_path_beats_independent_minimum(self):
        paths=[]
        for f in range(3):
            paths.extend([dict(id=f*2,family=f,nodes=[f'private-{f}'],lhs_atoms=1),
                          dict(id=f*2+1,family=f,nodes=['shared-a','shared-b'],lhs_atoms=2)])
        with tempfile.TemporaryDirectory() as d:
            r=solve(paths,[],Path(d),weights=(1,0,0))
        self.assertEqual(r['selected'],[1,3,5])
        self.assertEqual(r['objective'],2)

    def test_hyperedge_cost_matches_exhaustive_optimum(self):
        # Three endpoints: two parents and a child. Joint transition cost makes
        # the larger shared representation win; endpoint-local minima lose.
        paths = [dict(id=2*f+i, family=f, nodes=([f'private-{f}'] if i == 0 else ['shared-a','shared-b']))
                 for f in range(3) for i in range(2)]
        opts = []
        for ps in itertools.product(paths[:2], paths[2:4], paths[4:]):
            ids = [p['id'] for p in ps]
            opts.append(dict(id='impl_'+'_'.join(map(str, ids)), requires=ids,
                             search_atoms=2 if all(i % 2 for i in ids) else 7, update_actions=1))
        def objective(opt):
            nodes = {n for p in paths if p['id'] in opt['requires'] for n in p['nodes']}
            return len(nodes) + opt['search_atoms'] + opt['update_actions']
        expected = min(opts, key=objective)
        with tempfile.TemporaryDirectory() as d:
            result = solve(paths, [dict(id=0, options=opts)], Path(d))
        self.assertEqual(result['selected'], [1,3,5])
        self.assertEqual(result['implementations'], [expected['id']])
        self.assertEqual(result['objective'], objective(expected))
        self.assertEqual(result['bound'], objective(expected))

    def test_missing_implementation_forbids_incompatible_endpoint_tuple(self):
        paths = [dict(id=i, family=i//2, nodes=[str(i)]) for i in range(6)]
        opts = [dict(id='only_legal_impl', requires=[1,2,5], search_atoms=2, update_actions=1)]
        with tempfile.TemporaryDirectory() as d:
            result = solve(paths, [dict(id=0, options=opts)], Path(d))
        self.assertEqual(result['selected'], [1,2,5])


class NativePipeline(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='spine-test-')
        cls.folder=Path(cls.tmp.name)
        cls.outputs={}
        for name in ('assoc','union','multiple_types','default_ruleset','merged_ports','dual_parents','nested_parents','shared_family_graft','relation_fallback','function_fallback'):
            output=cls.folder/name
            subprocess.run([__import__('sys').executable,'-m','tools.cell_spine.compiler',
                '--source',f'experiments/cell_spine/fixtures/{name}.egg','--output',str(output),'--skip-build'],cwd=ROOT,check=True,
                stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            cls.outputs[name]=output

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_real_analysis_and_optimal_selection(self):
        d=json.loads((self.outputs['assoc']/'report.json').read_text())
        self.assertEqual(d['status'],'compiled')
        self.assertEqual(d['plan']['status'],'optimal')
        self.assertTrue(any(t['parents'] for t in d['transitions']))
        self.assertGreater(len(d['paths']),len(d['plan']['selected']))

    def test_isolation_and_full_state_equivalence(self):
        for name in ('assoc','union','multiple_types','default_ruleset','merged_ports','dual_parents','nested_parents','shared_family_graft'):
            report=json.loads((self.outputs[name]/'validation.json').read_text())
            for case in report['cases']:
                self.assertEqual(case['physical_original_rows_before_export'],0)
                self.assertEqual(case['comparison']['status'],'equivalent')

    def test_functions_and_relations_use_verified_stage_compression(self):
        for name in ('relation_fallback','function_fallback'):
            out=self.outputs[name]
            d=json.loads((out/'report.json').read_text())
            self.assertEqual(d['status'],'compiled_stages')
            self.assertFalse(d['generalizes_bindings'])
            self.assertTrue((out/'selection.lp').is_file())
            v=json.loads((out/'validation.json').read_text())
            self.assertTrue(v['cases'])
            self.assertTrue(all(c['comparison']['status']=='equivalent' for c in v['cases']))

    def test_invalid_union_is_rejected(self):
        output=self.outputs['assoc']
        manifest=json.loads((output/'work/validation.json').read_text())
        case=next(c for c in manifest['cases'] if c['name']=='path-0')
        case['compressed']+='\n(union (Spine-RipenInput 0) (Spine-RipenInput 1))\n'
        manifest['cases']=[case]
        path=self.folder/'bad.json'; path.write_text(json.dumps(manifest))
        result=subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/'bad-report.json'],
                              capture_output=True,text=True)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('full closure mismatch',result.stderr)

    def test_ordinary_materialization_is_rejected(self):
        output=self.outputs['assoc']
        manifest=json.loads((output/'work/validation.json').read_text())
        case=manifest['cases'][0]
        case['compressed']+='\n(Leaf "unexpected")\n'
        manifest['cases']=[case]
        path=self.folder/'expanded.json';path.write_text(json.dumps(manifest))
        result=subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/'expanded-report.json'],
                              capture_output=True,text=True)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('original constructors materialized',result.stderr)

    def test_late_boundary_input_uses_existing_parent(self):
        output=self.outputs['assoc']
        lib=(output/'library.egg').read_text()
        # The generated rule recognizes a parent Summary, then its new coarse
        # context. The old context is installed and run before the new fact.
        case=dict(name='late-boundary',reference='''
(datatype Term (Leaf String) (Plus Term Term))
(ruleset algebra)
(rewrite (Plus a b) (Plus b a) :ruleset algebra)
(rewrite (Plus (Plus a b) c) (Plus a (Plus b c)) :ruleset algebra)
(Plus (Leaf "a") (Leaf "b"))
(run-schedule (saturate algebra))
(Plus (Plus (Leaf "b") (Leaf "a")) (Leaf "c"))
(run-schedule (saturate algebra))
''',compressed=lib+'''
(Spine-Plus (Spine-Leaf "a") (Spine-Leaf "b"))
(run-schedule (saturate Spine-Run))
(check (Spine-Summary-0 (Spine-Plus (Spine-Leaf "a") (Spine-Leaf "b")) (Spine-Leaf "a") (Spine-Leaf "b")))
(Spine-Plus (Spine-Plus (Spine-Leaf "b") (Spine-Leaf "a")) (Spine-Leaf "c"))
(run-schedule (saturate Spine-Run))
''',restore='(run-schedule (saturate Spine-Export))',expected_summary='Spine-Summary-1')
        manifest=dict(constructors=['Leaf','Plus','RipenInput'],cases=[case])
        path=self.folder/'late.json';path.write_text(json.dumps(manifest))
        subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/'late-report.json'],check=True,stdout=subprocess.DEVNULL)

    def test_noninjective_boundary_bindings(self):
        output=self.outputs['assoc']
        manifest=json.loads((output/'work/validation.json').read_text())
        case=next(c for c in manifest['cases'] if c['name']=='source')
        case['reference']=Path(case.pop('reference_path')).read_text().replace('"b"','"a"')
        case['compressed']=case['compressed'].replace('"b"','"a"')
        case['restore']=case['restore'].replace('"b"','"a"')
        manifest['cases']=[case]
        path=self.folder/'aliases.json';path.write_text(json.dumps(manifest))
        subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/'aliases-report.json'],check=True,stdout=subprocess.DEVNULL)

    def test_original_rule_execution_is_rejected(self):
        output=self.outputs['assoc']
        manifest=json.loads((output/'work/validation.json').read_text())
        case=manifest['cases'][0]
        case['restore']+='\n(run algebra 1)\n'
        manifest['cases']=[case]
        path=self.folder/'rules.json';path.write_text(json.dumps(manifest))
        result=subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/'rules-report.json'],capture_output=True,text=True)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('may not execute the original ruleset',result.stderr)

    def test_merged_ports_remain_distinct_interfaces_and_union_before_export(self):
        output=self.outputs['merged_ports']
        manifest=json.loads((output/'work/validation.json').read_text())
        case=next(c for c in manifest['cases'] if c['name']=='source')
        case['compressed']+='\n(check (= a b)) (check (= root a))\n'
        manifest['cases']=[case]
        path=self.folder/'visible-unions.json';path.write_text(json.dumps(manifest))
        subprocess.run([ROOT/'target/release/examples/spine_validate',path,self.folder/'visible-unions-report.json'],check=True,stdout=subprocess.DEVNULL)

    def validate_extra(self, fixture, name, reference, compressed, restore='(run-schedule (saturate Spine-Export))'):
        output = self.outputs[fixture]
        base = json.loads((output/'work/validation.json').read_text())
        case = dict(name=name, reference=reference, compressed=compressed, restore=restore)
        manifest = dict(constructors=base['constructors'], library=base['library'], cases=[case])
        path = self.folder/f'{name}.json'
        path.write_text(json.dumps(manifest))
        subprocess.run([ROOT/'target/release/examples/spine_validate', path, self.folder/f'{name}-report.json'],
                       check=True, stdout=subprocess.DEVNULL)

    def test_two_parent_rule_is_only_route_and_has_no_raw_constructor_join(self):
        out = self.outputs['dual_parents']
        report = json.loads((out/'report.json').read_text())
        self.assertEqual(report['status'], 'compiled')  # Not a concrete stage archive.
        target = next(t for t in report['transitions'] if len(t['parents']) == 2)
        self.assertEqual(len(target['body']), 2)
        self.assertTrue(all(s.startswith('(Spine-Summary-') for s in target['body']))
        self.assertEqual(sum(h.startswith('(Spine-Summary-') for h in target['head']), 1)
        self.assertEqual(sum(h.startswith('(subsume ') for h in target['head']), 2)
        self.assertEqual([t for t in report['transitions'] if t['target_family'] == target['target_family']], [target])
        self.assertEqual(len({p['unit'] for p in target['parents']}), 2)
        self.assertTrue(all(len(p['roots']) == 1 for p in target['parents']))
        for edge in json.loads((out/'transitions.json').read_text()):
            opt = next(o for o in edge['options'] if o['id'] in report['plan']['implementations'])
            self.assertTrue(set(opt['requires']) <= set(report['plan']['selected']))

    def test_bigger_spine_can_be_a_parent_again(self):
        out = self.outputs['nested_parents']
        report = json.loads((out/'report.json').read_text())
        self.assertEqual(report['status'], 'compiled')
        transitions = [t for t in report['transitions'] if len(t['parents']) == 2]
        self.assertEqual(len(transitions), 2)
        first, last = sorted(transitions, key=lambda t: t['target_unit'])
        self.assertIn(first['target_family'], [p['family'] for p in last['parents']])
        self.assertTrue(all(len(t['body']) == 2 and sum(h.startswith('(subsume ') for h in t['head']) == 2 for t in transitions))
        compressed = f"""
(let x (Spine-Leaf "x"))
(Spine-A x) (Spine-B x)
(run-schedule (saturate Spine-Run))
(check (Spine-Summary-{first['target_family']} a b x))
(fail (check (Spine-Summary-{last['target_family']} a b c x)))
(Spine-C x)
(run-schedule (saturate Spine-Run))
(check (Spine-Summary-{last['target_family']} a b c x))
(fail (check (Spine-Joined x)))
(fail (check (Spine-Final x)))
"""
        reference = (ROOT/'experiments/cell_spine/fixtures/nested_parents.egg').read_text()
        self.validate_extra('nested_parents', 'nested-hyperedge', reference, compressed)

    def test_equality_preconditions_survive_factoring(self):
        out = self.outputs['dual_parents']
        cs, _ = load_candidates(out/'analysis', out/'work')
        by_id = {c['id']: c for c in cs}
        child = copy.deepcopy(next(c for c in cs if c['origin']['composition']['parts'] == [0,1]))
        parent = copy.deepcopy(by_id[0])
        # Deliberately strengthen a parent's seed. Node containment alone must
        # not admit this parent when the child's entry lacks the new equality.
        root = parent['acts'][0]['expr']
        guard = dict(kind='union', args=[root, root['args'][0]])
        parent['acts'].append(guard)
        by_id[0] = parent
        self.assertEqual([w['parent']['id'] for w in parent_wirings(child, child['params'], by_id)], [1])
        child['acts'].append(guard)
        ws = parent_wirings(child, child['params'], by_id)
        self.assertEqual([w['parent']['id'] for w in ws], [0,1])
        opt = lower_option(child, child, ws, {c['family']: c for c in by_id.values()})
        self.assertIn('(= parent_0_root_0 p0)', opt['body'])

    def test_mismatched_ports_block_then_union_enables_two_parent_transition(self):
        # A and B Summaries exist separately. The target cannot be created just
        # from family identity: its shared x must match, also after rebuild.
        reference = """
(datatype Term (Leaf String) (A Term) (AA Term) (B Term) (BB Term) (Joined Term))
(ruleset demo)
(rewrite (A x) (AA x) :ruleset demo)
(rewrite (B x) (BB x) :ruleset demo)
(rule ((AA x) (BB x)) ((Joined x)) :ruleset demo)
(let x (Leaf "x")) (let y (Leaf "y"))
(A x) (B y)
(run-schedule (saturate demo))
(fail (check (Joined x)))
(union x y)
(run-schedule (saturate demo))
(check (Joined x))
"""
        compressed = """
(let x (Spine-Leaf "x")) (let y (Spine-Leaf "y"))
(Spine-A x)
(run-schedule (saturate Spine-Run))
(check (Spine-Summary-0 (Spine-A x) x))
(fail (check (Spine-Summary-2 a b p)))
(Spine-B y)
(run-schedule (saturate Spine-Run))
(check (Spine-Summary-1 (Spine-B y) y))
(fail (check (Spine-Summary-2 a b p)))
(union x y)
(run-schedule (saturate Spine-Run))
(check (Spine-Summary-2 (Spine-A x) (Spine-B y) x))
(fail (check (Spine-AA x)))
(fail (check (Spine-BB x)))
(fail (check (Spine-Joined x)))
"""
        self.validate_extra('dual_parents', 'union-enables-dual', reference, compressed)

    def test_same_family_keeps_two_instances_and_external_graft(self):
        out = self.outputs['shared_family_graft']
        report = json.loads((out/'report.json').read_text())
        self.assertEqual(report['status'], 'compiled')
        t = next(t for t in report['transitions'] if len(t['parents']) == 2)
        self.assertEqual(len({p['family'] for p in t['parents']}), 1)
        self.assertEqual(len({p['path'] for p in t['parents']}), 1)
        self.assertEqual(len({p['unit'] for p in t['parents']}), 2)
        self.assertNotEqual(t['parents'][0]['slots'], t['parents'][1]['slots'])
        self.assertIn('(Spine-Gate p0 p2)', ' '.join(t['body']))
        reference = (ROOT/'experiments/cell_spine/fixtures/shared_family_graft.egg').read_text()
        reference = reference.replace('(run-schedule', '(Gate (Leaf "c") (Leaf "a"))\n(run-schedule', 1)
        compressed = """
(let a (Spine-Leaf "a")) (let b (Spine-Leaf "b")) (let c (Spine-Leaf "c"))
(Spine-A a b)
(run-schedule (saturate Spine-Run))
(fail (check (Spine-Summary-1 r0 r1 g x y z)))
(Spine-A b c)
(run-schedule (saturate Spine-Run))
(fail (check (Spine-Summary-1 r0 r1 g x y z)))
(Spine-Gate c a)
(run-schedule (saturate Spine-Run))
(fail (check (Spine-Summary-1 r0 r1 g x y z)))
(Spine-Gate a c)
(run-schedule (saturate Spine-Run))
(check (Spine-Summary-1 (Spine-A a b) (Spine-A b c) (Spine-Gate a c) a b c))
(fail (check (Spine-AA x y)))
(fail (check (Spine-Joined x z)))
"""
        self.validate_extra('shared_family_graft', 'same-family-graft', reference, compressed)


if __name__=='__main__':
    unittest.main()

import copy
import json
from pathlib import Path
import random
import subprocess
import tempfile
from types import SimpleNamespace
import unittest

from . import demo
from .compiler import FamilyProgram, Tape, MIN, MAX, affine
from .runtime import CPU, GPU, evaluate, explore, materialize


class Families(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory(prefix='parameter-cells-')
        cls.addClassCleanup(cls.tmp.cleanup)
        cls.out = Path(cls.tmp.name)
        cls.report = demo.run(cls.out/'audit',gpu=False,skip_build=True,math=False)
        cls.ast = json.loads((cls.out/'audit/ast.json').read_text())
        cls.program = FamilyProgram(cls.ast)
        cls.serial = 0

    def compile(self, source):
        type(self).serial += 1
        path = self.out/f'case-{self.serial}.egg'; path.write_text(source)
        return FamilyProgram(demo.parse(path,path.with_suffix('.json')))

    def test_native_full_bindings_and_graphs_before_and_after_smooth(self):
        for p in self.report['phases']:
            self.assertEqual(len(p['audit']['checks']),2)
            for c in p['audit']['checks']:
                self.assertTrue(c['exact_equal'])
                self.assertEqual(c['logical_bindings'],p['summary']['logical_bindings'])
            self.assertFalse(p['summary']['pending'])

    def test_new_values_reuse_the_same_six_templates(self):
        self.assertEqual(self.report['baked_templates'],list(range(6)))
        self.assertEqual(self.report['phases'][1]['summary']['by_family']['Fib'],9)
        self.assertEqual(self.report['phases'][2]['summary']['by_family']['Fib'],10)
        self.assertEqual(self.report['recursive_components'],[['Fib'],['StepA','StepB']])

    def test_fib_is_shared_dag_with_two_nonroot_child_positions(self):
        p = self.program
        r = explore(p,[('Fib',[5])],CPU(p))
        self.assertEqual(len(r['instances']),6)
        self.assertEqual(r['child_occurrences'],8)
        first = r['effects'][0]
        self.assertEqual(first['children'],[
            dict(family='Fib',path=[0],parameters=[4]),dict(family='Fib',path=[1],parameters=[3])])
        self.assertEqual(p.templates[0]['children'][0]['affine_rows'],[[1,-1]])
        self.assertEqual(p.templates[0]['children'][1]['affine_rows'],[[1,-2]])

    def test_guard_local_definitions_compose_without_erasing_conditions(self):
        c = self.report['composition']
        self.assertEqual(c['children'][0]['affine_rows'],[[1,-3]])
        self.assertEqual(len(c['guards']),2)
        self.assertEqual(self.program.templates[3]['binding_names'],['m','k'])
        self.assertEqual(affine(self.program.templates[3]['bindings'][1],1),[1,-1])
        for n in range(-1,12):
            tape = Tape(); guards = [tape.add(e) for e in c['guards']]; end = len(tape.ops)
            output = [tape.add(c['children'][0]['parameters'][0])]
            t = dict(tape=tape.ops,guard_end=end,guard_ids=guards,output_ids=output)
            status,values = evaluate(t,[n])
            self.assertEqual(status,int(n>2))
            if status: self.assertEqual(values,[n-3])

    def test_composing_one_branch_preserves_the_other_branch(self):
        c = self.program.compose(0,0,0)
        self.assertEqual([(x['path'],x['affine_rows']) for x in c['children']],
                         [([1],[[1,-2]]),([0,0],[[1,-2]]),([0,1],[[1,-3]])])
        self.assertEqual(len(c['guards']),2)

    def test_native_oracle_rejects_wrong_numeric_child_update(self):
        r = explore(self.program,[('Fib',[3])],CPU(self.program))
        r['effects'][0]['output'][-1] += 1
        with self.assertRaises(AssertionError):
            demo.audit(self.out/'negative-update',self.program,[('Fib',[3])],r,
                       demo.ROOT/'target/release/examples/native_math_baseline')

    def test_materialization_retains_intermediate_facts_and_no_recursive_rules(self):
        r = explore(self.program,[('StepA',[6])],CPU(self.program))
        text = materialize(self.program,r,[('StepA',[6])])
        self.assertIn('(union (StepA 6) (StepB 5))',text)
        self.assertIn('(union (StepB 5) (StepA 3))',text)
        self.assertNotIn('(rewrite',text)
        self.assertNotIn('(run',text)

    def test_missing_bake_certificate_is_pending_not_rejected_match(self):
        r = explore(self.program,[('Fib',[5])],CPU(self.program),allowed={1})
        self.assertFalse(r['complete']); self.assertEqual(r['pending'][0]['reason'],'unbaked-template')
        with self.assertRaisesRegex(ValueError,'unfinished'): materialize(self.program,r,[])

    def test_two_parameters_repeated_child_and_unfamiliar_names_are_generic(self):
        p = self.compile('''(datatype Math (Z i64) (Pair Math Math) (Walk i64 i64))
(rewrite (Walk n d) (Pair (Walk (- n d) d) (Walk (- n d) d)) :when ((> n d) (> d 0)))
(rewrite (Walk n d) (Z n) :when ((<= n d)))''')
        r = explore(p,[('Walk',[7,2])],CPU(p))
        self.assertEqual(len(r['instances']),4)
        self.assertEqual(r['child_occurrences'],6)
        self.assertEqual(p.templates[0]['children'][0]['affine_rows'],[[1,-1,0],[0,1,0]])
        # Shared expressions, not two separately evaluated copies of n-d.
        self.assertEqual(sum(op==3 for op,_,_ in p.templates[0]['tape']),1)

    def test_all_matching_alternatives_are_kept(self):
        p = self.compile('''(datatype Math (N i64) (F i64))
(rewrite (F n) (N n) :when ((> n 0)))
(rewrite (F n) (N (+ n 1)) :when ((> n 1)))''')
        r = explore(p,[('F',[3])],CPU(p))
        self.assertEqual(len(r['effects']),2)

    def test_changed_arithmetic_changes_template_and_binding(self):
        ast = copy.deepcopy(self.ast)
        ast['rewrites'][0]['rhs']['args'][1]['args'][0]['args'][1]['value'] = 3
        p = FamilyProgram(ast)
        self.assertEqual(p.templates[0]['children'][1]['affine_rows'],[[1,-3]])
        self.assertNotEqual(evaluate(p.templates[0],[5]),evaluate(self.program.templates[0],[5]))

    def test_alias_of_numeric_parameters_is_not_value_erasure(self):
        p = self.compile('''(datatype Math (N i64) (F i64 i64))
(rewrite (F x y) (N (- x y)) :when ((= x y)))''')
        self.assertEqual(evaluate(p.templates[0],[8,8])[0],1)
        self.assertEqual(evaluate(p.templates[0],[8,9])[0],0)

    def test_overflow_is_pending_and_false_guard_does_not_evaluate_rhs(self):
        p = self.compile('''(datatype Math (N i64) (F i64))
(rewrite (F n) (N (+ n 1)) :when ((> n 0)))''')
        self.assertEqual(evaluate(p.templates[0],[MAX]),[2,[]])
        self.assertEqual(evaluate(p.templates[0],[MIN]),[0,[]])
        r = explore(p,[('F',[MAX])],CPU(p))
        self.assertFalse(r['complete'])
        with self.assertRaisesRegex(ValueError,'int32'): CPU(p).batch([(0,[MAX+1])])

    def test_unknown_primitives_cyclic_definitions_and_hidden_generation_rejected(self):
        sources = [
            '(datatype Math (N i64) (F i64)) (rewrite (F n) (N (/ n 2)))',
            '(datatype Math (N i64) (F i64)) (rewrite (F n) (N a) :when ((= a (+ b 1)) (= b (+ a 1))))',
            '(datatype Math (N i64) (Add Math Math) (F i64)) (rewrite (F n) (N n)) (rewrite (Add a b) (F 3))',
        ]
        for source in sources:
            with self.assertRaises(ValueError): self.compile(source)


class MetalParameters(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory(prefix='parameter-metal-')
        cls.addClassCleanup(cls.tmp.cleanup)
        cls.out = Path(cls.tmp.name); cls.exe = cls.out/'gpu'
        subprocess.run(['swiftc','-O',demo.HERE/'gpu.swift','-o',cls.exe],check=True)

    def check(self, templates, tasks):
        p = SimpleNamespace(templates=templates)
        gpu = GPU(p,self.exe)
        try: actual = gpu.batch(tasks)
        finally: gpu.close()
        self.assertEqual(actual,CPU(p).batch(tasks))
        return actual

    def test_exact_integer_arithmetic_comparisons_and_overflow(self):
        ops = ['+','-','*','>','>=','<','<=','==','!=']; templates=[]
        for op in ops:
            tape = Tape(); output = tape.add((op,('slot',0),('slot',1)))
            templates.append(dict(arity=2,tape=tape.ops,guard_end=0,guard_ids=[],output_ids=[output]))
        pairs = [(MIN,-1),(MIN,1),(MAX,1),(MAX,MAX),(MIN,MIN),(0,MIN),(0,0),
                 ((1<<24)+1,1),((1<<24)+1,1<<24),(-46341,46341)]
        rng = random.Random(27)
        pairs += [(rng.randint(MIN,MAX),rng.randint(MIN,MAX)) for _ in range(100)]
        tasks = [(i,list(pair)) for i in range(len(ops)) for pair in pairs]
        answers = self.check(templates,tasks)
        self.assertEqual(answers[7],[1,[(1<<24)+2]])
        self.assertTrue(any(s==2 for s,_ in answers))

    def test_guard_blocks_overflow_and_renamed_program_runs_same_device(self):
        path = self.out/'rules.json'; ast = demo.parse(demo.HERE/'rules.egg',path)
        p = FamilyProgram(ast); gpu = GPU(p,self.exe)
        try:
            for seeds in [[('Fib',[8])],[('StepA',[11]),('StepB',[10])],[('Fib',[1]),('Fib',[0])]]:
                self.assertEqual(explore(p,seeds,gpu),explore(p,seeds,CPU(p)))
            self.assertEqual(gpu.batch([(0,[MIN]),(1,[MIN])]),[[0,[]],[1,[MIN,MIN]]])
        finally: gpu.close()


if __name__=='__main__': unittest.main()

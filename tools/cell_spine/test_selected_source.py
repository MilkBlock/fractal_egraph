import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from .compiler import ROOT
from .selected_source import compile_source


class ParameterizedSourceMarkers(unittest.TestCase):
    def test_ordered_merge_keeps_original_rule_frontiers_and_declares_runtime_markers(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out') as temp:
            out=Path(temp);source=out/'ordered.egg'
            code=(ROOT/'experiments/parameterized_source_markers/counter.egg').read_text()
            code=code.replace('(ruleset s)','(function picked (E) E :merge new)\n(ruleset s)')
            source.write_text(code)
            target,report=self.compile(source,out,'State')
            self.assertTrue(report['source_preserved'])
            self.assertEqual(report['source_lowering_obligation']['functions'],['picked'])
            self.assertEqual(report['lowered_queries'],[])
            self.assertEqual(report['runtime_marker_consumers'],2)
            self.assertEqual(target.read_text().split('\n',1)[1],code)

    def test_rewritten_rules_keep_original_ruleset_order(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out') as temp:
            out=Path(temp);source=out/'input.egg'
            code=(ROOT/'experiments/parameterized_source_markers/counter.egg').read_text()
            code=code.replace('(ruleset s)','(relation Extra (E i64))\n(ruleset s)')
            needle='(rule ((= state (State x n)) (= port (Port x)) (= len (limit x)) (> n 0)'
            code=code.replace(needle,'(rule ((State x n)) ((Extra x n)) :ruleset s :name "untouched")\n'+needle)
            source.write_text(code);self.compile(source,out,'State')
            layout=out/'selected.json';d=json.loads(layout.read_text())
            d['rules']=[r for r in d['rules'] if r['rule']!='untouched'];layout.write_text(json.dumps(d))
            compile_source(source,layout,out/'partial',joint=False);target=out/'partial/source_sk_mk.egg'
            ast=out/'result.json'
            subprocess.run([ROOT/'target/analyzer/release/examples/lhs_ast',target,ast],check=True,capture_output=True)
            data=json.loads(ast.read_text())
            self.assertEqual([r['name'] for r in data['rules'] if r['ruleset']=='s'],['advance','untouched','observe'])

    def compile(self,source,out,root):
        native=out/'native.json'
        subprocess.run([ROOT/'target/analyzer/release/examples/scheduled_spine',source,native,'--all-schedules'],check=True,capture_output=True)
        r=json.loads(native.read_text());selected=[]
        for q in r['queries']:
            if 'plans' not in q:continue
            p=copy.deepcopy(next(p for p in q['plans'] if p['shape']['atoms'][p['seed']]['table']==root))
            p.update(prefix_len=len(p['steps']),carried_slots=list(range(len(p['shape']['slots']))),shared_marker=True)
            selected.append(dict(ruleset=q['ruleset'],rule=q['rule'],root=root,plan=p))
        file=out/'selected.json'
        file.write_text(json.dumps(dict(schema='cell-boundary-programs/v2',setup=r['preserved_commands'][:r['setup_commands']],rules=selected)))
        report=compile_source(source,file,out/'compiled',joint=False)
        return out/'compiled/source_sk_mk.egg',report

    def run_native(self,path):
        p=subprocess.run([ROOT/'target/analyzer/release/examples/native_math_baseline',path],capture_output=True,text=True)
        self.assertEqual(p.returncode,0,p.stdout+p.stderr)

    def test_private_zero_prefix_remains_a_runtime_plan_without_fake_source_marker(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out') as temp:
            out=Path(temp);source=ROOT/'experiments/parameterized_source_markers/counter.egg'
            self.compile(source,out,'State')
            layout=out/'selected.json';selected=json.loads(layout.read_text())
            for spec in selected['rules']:spec['plan'].update(prefix_len=0,carried_slots=[],shared_marker=False)
            layout.write_text(json.dumps(selected))
            report=compile_source(source,layout,out/'private',joint=False)
            self.assertEqual(report['markers'],[])
            self.assertEqual(report['publish_actions'],0)
            self.assertEqual(report['runtime_rule_count'],len(selected['rules']))
            self.assertEqual((out/'private/source_sk_mk.egg').read_text().split('\n',1)[1],source.read_text())
            self.assertTrue(report['inline_cell_declarations'])
            runtime=json.loads((out/'private/runtime-layout.json').read_text())
            self.assertEqual(len(runtime['rules']),len(selected['rules']))

    def test_two_if_consumers_share_the_full_spine_but_not_global_state(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out') as temp:
            temp=Path(temp);source=temp/'input.egg'
            source.write_text((ROOT/'experiments/observed_transfer_queries/if-two-consumers.egg').read_text()+'\n'+
                (ROOT/'experiments/observed_transfer_queries/if-global-updates.egg').read_text())
            target,report=self.compile(source,temp,'If')
            self.assertEqual(len(report['lowered_queries']),2)
            self.assertEqual(len({q['marker'] for q in report['lowered_queries']}),1)
            self.assertEqual([q['prefix_atoms'] for q in report['lowered_queries']],[10,10])
            self.assertEqual(sorted(q['residual_atoms'] for q in report['lowered_queries']),[0,1])
            for m in report['markers']:
                self.assertNotIn('DUMMYCTX',[a['table'] for a in m['definition']['body']['atoms']])
            self.run_native(source);self.run_native(target)

    def test_numeric_successor_is_published_without_rescanning_its_spine(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'out') as temp:
            temp=Path(temp);source=ROOT/'experiments/parameterized_source_markers/counter.egg'
            target,report=self.compile(source,temp,'State')
            code=target.read_text();self.assertEqual(report['publish_actions'],1)
            advance=next(l for l in code.splitlines() if l.startswith('(rule ((CellSelectedSpine') and ':name "advance"' in l)
            self.assertEqual(advance.count('(+ '),1)
            self.assertEqual(advance.count('(State '),1)
            self.assertIn('(set (CellSelectedSpine',advance)
            self.assertIn('(limit ',advance)
            self.run_native(source);self.run_native(target)
            # Controlled fragment: after bootstrapping, no external writes or
            # unions occur until the limit change. Its second step MUST come
            # from the producer's new marker even with the repair run removed.
            short=code[:code.index('(delete (limit')]
            first=True;lines=[]
            for l in short.splitlines():
                if l.startswith('(run-schedule'):
                    if first:first=False
                    else:l=l.replace('(saturate cell-selected-spines)','(seq)')
                lines.append(l)
            no_rescan=temp/'no-rescan.egg';no_rescan.write_text('\n'.join(lines)+'\n')
            self.run_native(no_rescan)

import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from .compiler import ROOT
from .selected_source import compile_source


class ParameterizedSourceMarkers(unittest.TestCase):
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

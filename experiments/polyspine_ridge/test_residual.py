"""Residual semantics, including alternatives, aliasing and unseeded cycles."""
import copy
import json
import os
from pathlib import Path
import subprocess
import struct
import tempfile
import unittest
import numpy as np
from residual import compile_recipes, pack_context, project, frozen_closure, incidence
from residual_capture import write_packet


def sort(n):return {'name':n,'kind':'eq'}
def arg(n,t='Expr'):return {'var':n,'sort':sort(t)}
def atom(t,*args):return {'call':{'table':t},'args':list(args)}
def val(n,t='Expr'):return {'sort':t,'eq':n}
def row(t,*v):return {'table':t,'values':list(v),'subsumed':False}


def fixture():
    tables=[{'name':'Pair','kind':'constructor','schema':[sort('Expr')]*3},
            {'name':'Ready','kind':'constructor','schema':[sort('Expr'),sort('___ReadySort')]}]
    rule={'name':'pair-ready','body':[atom('Pair',arg('a'),arg('b'),arg('root')),
                                    atom('Ready',arg('a'),arg('ra','___ReadySort')),
                                    atom('Ready',arg('b'),arg('rb','___ReadySort'))],
          'head':[{'op':'let','dst':'out','call':{'table':'Ready'},'args':[arg('root')]}]}
    core={'tables':tables,'rulesets':[{'name':'test','rules':[rule]}]}
    snap={'rows':[row('Pair',val('a'),val('b'),val('root')),
                  row('Pair',val('c'),val('d'),val('root')),
                  row('Ready',val('a'),val('ra','___ReadySort')),
                  row('Ready',val('d'),val('rd','___ReadySort'))]}
    return core,snap


def context(core,snap,key='root'):
    p=compile_recipes(core,'Ready','test')
    return pack_context(core,snap,[('Expr','eq',key)],p)


class ResidualTests(unittest.TestCase):
    @unittest.skipUnless(os.environ.get('POLYSPINE_SCORE_WORKER'),'optional GPU numeric-tail packet')
    def test_numeric_tail_and_constant_head_routing(self):
        with tempfile.TemporaryDirectory() as d:
            d=Path(d)
            with (d/'input.bin').open('wb') as f:
                f.write(b'SPINE004');f.write(struct.pack('<I',2))
                for constant in [True,False]:
                    f.write(struct.pack('<3I',3,512,1))
                    for val in [0,.5,1]:
                        f.write(struct.pack('<2I',0,0))
                        tail=np.zeros(32,dtype='<f2');tail[0]=val;f.write(tail.tobytes())
                    w=np.zeros(512,dtype='<f2')
                    if not constant:w[-32]=512
                    f.write(w.tobytes());f.write(np.array([.25 if constant else 0],'<f4').tobytes())
            np.array([.25,.25,.25,0,.5,1],'<f4').tofile(d/'reference.bin')
            subprocess.run([os.environ['POLYSPINE_SCORE_WORKER'],str(d/'input.bin'),str(d/'reference.bin'),str(d/'report.json')],check=True,stdout=subprocess.DEVNULL)
            r=json.loads((d/'report.json').read_text());self.assertEqual(r['threshold_disagreements'],0);self.assertEqual(r['max_score_error'],0)

    def test_different_alternatives_cannot_supply_half_each(self):
        c=context(*fixture())
        self.assertFalse(project(c).any());self.assertFalse(frozen_closure(c)[0].any())

    def test_one_complete_alternative_is_enough(self):
        core,s=fixture();s['rows'].append(row('Ready',val('b'),val('rb','___ReadySort')))
        self.assertEqual(project(context(core,s))[0].tolist(),[0]+[1]*8)

    def test_cycle_without_seed_does_not_prove_itself(self):
        core,_=fixture();s={'rows':[row('Pair',val('root'),val('root'),val('root'))]}
        c=context(core,s);self.assertFalse(project(c).any());self.assertFalse(frozen_closure(c)[0].any())

    def test_known_seed_starts_recursive_chain(self):
        core,_=fixture();s={'rows':[row('Pair',val('a'),val('a'),val('b')),
                                   row('Pair',val('b'),val('b'),val('root')),
                                   row('Ready',val('a'),val('ra','___ReadySort'))]}
        self.assertEqual(project(context(core,s))[0].tolist(),[0,0]+[1]*7)

    def test_same_variable_anchor_constraint(self):
        core,s=fixture();r=core['rulesets'][0]['rules'][0]
        r['body'][0]['args'][1]=arg('a');r['body']=r['body'][:2]
        self.assertFalse(project(context(core,s)).any())
        s['rows'][0]['values'][1]=val('a')
        self.assertTrue(project(context(core,s))[0,1])

    def test_private_return_alias_constraint_rejected(self):
        core,_=fixture();core['rulesets'][0]['rules'][0]['body'][2]['args'][-1]=arg('ra','___ReadySort')
        p=compile_recipes(core,'Ready','test');self.assertEqual(len(p['recipes']),0);self.assertEqual(len(p['rejected']),1)

    def test_mutating_rule_not_silently_accepted(self):
        core,_=fixture();core['rulesets'][0]['rules'][0]['head'].append({'op':'union','args':[arg('a'),arg('b')]})
        self.assertEqual(len(compile_recipes(core,'Ready','test')['rejected']),1)

    def test_subsumed_seed_cannot_fire(self):
        core,s=fixture();s['rows'][0]['values'][1]=val('a');s['rows'][2]['subsumed']=True
        self.assertFalse(project(context(core,s)).any())

    @unittest.skipUnless(os.environ.get('POLYSPINE_RESIDUAL_WORKER'),'optional actual GPU differential fixture')
    def test_gpu_alternatives_and_alias(self):
        core,s=fixture();cases=[context(core,s)]
        s['rows'].append(row('Ready',val('b'),val('rb','___ReadySort')));cases.append(context(core,s))
        # Build a two-step chain with a non-root query to exercise forwarding.
        s={'rows':[row('Pair',val('a'),val('a'),val('b')),
                   row('Pair',val('b'),val('b'),val('root')),
                   row('Ready',val('a'),val('ra','___ReadySort'))]};cases.append(context(core,s))
        # Readiness counts premise occurrences (2/3), whereas Boolean closure
        # may deduplicate the same fact. Neither can turn this into a proof.
        core,s=fixture();core['rulesets'][0]['rules'][0]['body'].append(atom('Ready',arg('a'),arg('again','___ReadySort')))
        cases.append(context(core,s))
        with tempfile.TemporaryDirectory() as d:
            d=Path(d);frames=[]
            for i,c in enumerate(cases):
                write_packet(d/f'{i}.bin',c);frames.append({'cycle':i,'packet':f'{i}.bin'})
            (d/'capture.json').write_text(json.dumps({'program':cases[0]['program'],'frames':frames}))
            subprocess.run([os.environ['POLYSPINE_RESIDUAL_WORKER'],str(d/'capture.json'),str(d/'gpu'),str(Path(__file__).with_name('residual.metal'))],check=True,stdout=subprocess.DEVNULL)
            for i,c in enumerate(cases):
                got=np.fromfile(d/'gpu'/f'cycle-{i:02}-prestate.bin',dtype='<f4').reshape(-1,17)
                np.testing.assert_array_equal(got[:,[0,1,3,5,7,9,11,13,15]],project(c))
                state=c['seed'].copy();expected=np.zeros_like(got);expected[:,0]=state[c['queries']]
                for depth in range(1,9):
                    nxt=state.copy();fraction=np.zeros_like(state)
                    for t,deps in incidence(c,unique=False):
                        ready=sum(float(state[x]) for x in deps)/len(deps) if deps else 1.
                        fraction[t]=max(fraction[t],ready)
                        if ready==1:nxt[t]=1
                    expected[:,2*depth-1]=nxt[c['queries']];expected[:,2*depth]=fraction[c['queries']];state=nxt
                np.testing.assert_array_equal(got,expected)


if __name__=='__main__':unittest.main()

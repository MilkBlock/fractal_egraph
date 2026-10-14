"""Certified left/right graft interfaces, independent of a chosen history order."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from . import compiler as c, composition, takeover as t
from .test_composition import candidate,input_term


class BoundaryProofs(unittest.TestCase):
    def examples(self):
        x,y=input_term(7),input_term(9);a,b=input_term(11),input_term(12)
        parent=candidate(0,0,[t.call('Join',[x,y])],[x,y])
        target=candidate(1,1,[t.call('A',[t.call('Join',[a,b])])],[a,b])
        for q in (parent,target):
            q['boundary_positive']=True
            q['markers']={'RipenInput'}
            q['snapshot']['scope']={'semantics':'positive-constructor-equality/v1'}
            row=next(r for r in q['snapshot']['rows'] if r['op']=='Join')
            q['snapshot']['rows'].append(dict(row,args=list(reversed(row['args']))))
        return parent,target

    def test_closure_provides_both_orientations_of_one_canonical_seed(self):
        p,candidate_=self.examples();edges=[]
        composition.extend_boundaries([p,candidate_],[p,candidate_],edges)
        maps=[e['seed_cut']['parents'][0]['bindings'] for e in edges]
        self.assertEqual(len(maps),2)
        self.assertNotEqual(maps[0],maps[1])
        self.assertTrue(all(e['kind']=='certified_boundary_cut' for e in edges))

    def test_extra_parent_equality_or_environment_cannot_be_erased(self):
        p,target=self.examples();root=c.resolve(p['snapshot'],p['roots'][0]);port=c.resolve(p['snapshot'],input_term(7))
        p['acts'].append(dict(kind='union',args=[p['roots'][0],input_term(7)]))
        for row in p['snapshot']['rows']:
            row['args']=[port if x==root else x for x in row['args']]
            if row['result']==root:row['result']=port
        edges=[];composition.extend_boundaries([p,target],[p,target],edges)
        self.assertFalse(edges)
        p,target=self.examples();target['snapshot']['scope']['rules']=['different']
        edges=[];composition.extend_boundaries([p,target],[p,target],edges)
        self.assertFalse(edges)


class NativeDiamond(unittest.TestCase):
    def test_history_order_shared_family_and_union_guard(self):
        with tempfile.TemporaryDirectory(dir=c.ROOT/'out',prefix='graft-test-') as d:
            out=Path(d)/'run'
            r=subprocess.run([sys.executable,c.ROOT/'experiments/cell_spine/graft_diamond/probe.py','--output',out],
                             cwd=c.ROOT,capture_output=True,text=True)
            self.assertEqual(r.returncode,0,r.stdout+r.stderr)
            data=json.loads((out/'results.json').read_text())
            self.assertTrue(all(q['status']=='equivalent' for q in data['closure_checks'].values()))
            for kind in ('symmetric_compact','union_compact','delayed_compact'):
                for q in data[kind]['cases']:
                    self.assertEqual(q['comparison']['status'],'equivalent')
                    self.assertFalse(q['original_rules_executed'])
            owners=[{f for f,n in q['active_summary_tables'].items() if n} for q in data['symmetric_compact']['cases'][:2]]
            self.assertEqual(owners[0],owners[1])


if __name__=='__main__':unittest.main()

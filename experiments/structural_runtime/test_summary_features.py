import json
from pathlib import Path
import tempfile
import unittest
import summary_features as s

class SummaryTests(unittest.TestCase):
    def test_joint_binding_not_independent_column_flags(self):
        cols=lambda slots:[{'Slot':i,'sort':'T'} for i in slots]
        d=dict(name='joint',atoms=[dict(table='R',columns=cols([0,1,2])),dict(table='P',columns=cols([0,3])),dict(table='Q',columns=cols([1,3]))],sorts=['T']*4,carried=[0,2],identity='joint',dependencies=['R','P','Q'])
        v=lambda n:('Eq','T',str(n))
        tables={'R':[(v(1),v(2),v(3))],'P':[(v(1),v(4))],'Q':[(v(2),v(5))]}
        self.assertEqual(s.bindings(d,tables),set())
        tables['Q'].append((v(2),v(4)))
        self.assertEqual(s.bindings(d,tables),{tuple(v(i) for i in [1,2,3,4])})
        ts,vocab,_=s.tokens([d],tables,{'T'})
        self.assertEqual(ts[v(3)],{('summary','joint',2)})
        self.assertEqual(s.excluded([d],vocab,'P'),[0,1])
        self.assertEqual(s.excluded([d],vocab,'Pure'),[])

    def test_metadata_must_match_egg(self):
        a=dict(table='R',columns=[dict(Slot=i,sort='T') for i in range(2)])
        text='; @marker '+json.dumps(dict(id=0,body_key=json.dumps(dict(atoms=[a])),carried=[0,1]))+'\n(relation SharedSpine0 (T T))\n(rule ((= s1 (R s0))) ((SharedSpine0 s0 s1)) :ruleset cell-marker-definitions)\n'
        with tempfile.TemporaryDirectory() as t:
            p=Path(t)/'m.egg';p.write_text(text)
            self.assertEqual(len(s.load(p,{'R':['T','T']})),1)
            p.write_text(text.replace('(R s0)','(R s1)'))
            with self.assertRaises(ValueError):s.load(p,{'R':['T','T']})

if __name__=='__main__':unittest.main()

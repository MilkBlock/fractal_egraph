import unittest
from tools.cell_spine.pass3_markers import structural_fragments,canonical,vars_in
from tools.cell_spine.takeover import var,call

def atom(name,result,*args):return {'kind':'eq','args':[var(result),call(name,[var(x) for x in args])]}

class MarkerLocalityTests(unittest.TestCase):
    def fixture(self,bridge=True,names=('If','Get')):
        branch,read=names
        sig={branch:dict(kind='constructor',output='E'),read:dict(kind='constructor',output='E')}
        body=([atom(branch,'root','left','right')] if bridge else [])+[atom(read,'a','left','i'),atom(read,'b','right','i')]
        types={v:'i64' if v=='i' else 'E' for a in body for e in a['args'] for v in vars_in(e)}
        return dict(body=body,atom_count=len(body),types=types),sig
    def test_shared_integer_needs_the_consumer_structure(self):
        flat,sig=self.fixture();parts,sorts=structural_fragments(flat,sig,{(1,2)})
        self.assertEqual(parts,{(0,1,2)})
        norm=canonical(flat['body'],flat['types'],sorts);self.assertIsNotNone(norm)
        _,body,_,types=norm;typed={f'cell_spine_slot{i}':s for i,s in enumerate(types)}
        # Every materialized prefix is connected through an equality identity.
        seen=set()
        for a in body:
            now={v for e in a['args'] for v in vars_in(e) if typed[v] in sorts}
            if seen:self.assertTrue(now & seen)
            seen.update(now)
    def test_no_bridge_leaves_original_query(self):
        flat,sig=self.fixture(False);parts,_=structural_fragments(flat,sig,{(0,1)})
        self.assertEqual(parts,set())
    def test_policy_does_not_depend_on_operator_names(self):
        flat,sig=self.fixture(names=('JoinContext','ReadPort'))
        self.assertEqual(structural_fragments(flat,sig,{(1,2)})[0],{(0,1,2)})
    def test_shared_eclass_does_not_need_shared_scalar(self):
        flat,sig=self.fixture(False)
        flat['body'][1]=atom('Get','b','left','j');flat['types']['j']='i64'
        self.assertEqual(structural_fragments(flat,sig,{(0,1)})[0],{(0,1)})

if __name__=='__main__':unittest.main()

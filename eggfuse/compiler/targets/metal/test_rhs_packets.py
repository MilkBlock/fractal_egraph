import unittest
from .rhs_packets import classify,callback_tables

EQ=dict(name='E',kind='eq');INT=dict(name='i64',kind='i64')
def constructor(name):return dict(name=name,kind='constructor',schema=[EQ,EQ],merge=None)
def function(merge,expr=None):return dict(name='F',kind='function',schema=[EQ,EQ],merge=merge,merge_expr=expr)
A=constructor('A');B=constructor('B')
# Load binding slot 0, construct A, feed its result into B.
DAG=[[0,0,0,2,0],[3,1,0,0,0],[0,0,0,1,1],[3,2,1,0,0]]
class Packets(unittest.TestCase):
    def test_nested_depth(self):
        x,why=classify(DAG,1,[A,B]);self.assertIsNone(why);self.assertEqual(x['depths'],[0,1]);self.assertFalse(x['tail'])
    def test_frozen_operand_not_rhs_alias(self):
        x,_=classify(DAG+[[0,0,0,2,1],[3,3,1,0,0]],2,[A,B]);self.assertEqual(x['depths'],[0,1,0])
    def test_callback_disjointness(self):
        f=function('(B old new)',dict(call=dict(table='B'),args=[dict(var='old'),dict(var='new')]))
        good=DAG[:2]+[[0,0,0,2,0],[0,1,0,1,1],[4,0,2,0,0]]
        x,why=classify(good,1,[A,B,f]);self.assertIsNone(why);self.assertEqual(x['tail_rows'],2);self.assertEqual(x['tail_unions'],1)
        self.assertEqual(classify(DAG+good[-3:],1,[A,B,f])[1],'callback observes moved table')
    def test_arbitrary_callbacks_not_moved(self):
        f=function('(+ old new)',dict(call=dict(primitive='+'),args=[dict(var='old'),dict(var='new')]))
        self.assertIsNone(callback_tables(f,[A,f]))
    def test_old_may_fail(self):
        self.assertIsNone(callback_tables(function('old'),[A]))
    def test_missing_function_read_not_preexecuted(self):
        self.assertEqual(classify([[3,1,2,0,0]]+DAG,1,[A,B,function('new')])[1],'mutable RHS function read')
    def test_masking_is_not_reordered(self):
        self.assertIsNone(classify(DAG+[[5,0,0,0,0]],1,[A,B])[0])
    def test_numeric_use_of_local_identity_rejected(self):
        self.assertEqual(classify(DAG+[[0,0,0,1,1],[6,4,0,0,0]],1,[A,B])[1],'arithmetic consumes a constructor identity')
    def test_union_tail_retained(self):
        x,_=classify(DAG+[[0,0,0,1,1],[0,1,0,1,2],[2,0,0,0,0]],1,[A,B]);self.assertEqual(x['tail_unions'],1)
if __name__=='__main__':unittest.main()

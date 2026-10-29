import copy,unittest
from .regions import verify_repeat


def record():
    s=dict(meta=[1,2,2,1,0,1,1,0,2],state=[0],rows=[[0,0,2,1,7]],parents=[0,1],fact_parents=[0,1],sets=[2,2],watermarks=[1],
           batch_statistics=[0]*14+[1,0],region_statistics=[0]*16,history=[[0,1,1,2,1,0,0,1,123,456,789,101]])
    return dict(complete=True,programs=[dict(instances=[s])])


class ReproducibilityTests(unittest.TestCase):
    def test_intermediate_divergence_cannot_hide_behind_equal_endpoints(self):
        a=record();b=copy.deepcopy(a);verify_repeat(a,b)
        b['programs'][0]['instances'][0]['history'][0][8]^=1
        with self.assertRaisesRegex(ValueError,'round history'):verify_repeat(a,b)

    def test_numeric_audit_and_complete_execution_are_required(self):
        a=record();b=copy.deepcopy(a)
        a['programs'][0]['instances'][0]['batch_statistics'][14]=0
        b['programs'][0]['instances'][0]['batch_statistics'][14]=0
        with self.assertRaisesRegex(ValueError,'audit'):verify_repeat(a,b)
        a=record();b=copy.deepcopy(a);b['complete']=False
        with self.assertRaisesRegex(ValueError,'incomplete'):verify_repeat(a,b)


if __name__=='__main__':unittest.main()

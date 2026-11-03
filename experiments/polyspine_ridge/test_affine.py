import json,os,struct,subprocess,tempfile,unittest
from pathlib import Path
import numpy as np
from affine_terms import compile_program,contexts,exact,metal_program,LO,HI
from affine_fit import reference_features
from study import Oracle


def fixture_core():
    v=lambda x:{'var':x}
    row=lambda n,*args:{'call':{'table':n},'args':list(map(v,args))}
    return {'rulesets':[{'name':'terms','rules':[{'name':'cost','body':[
        row('Bop','x','y','root'),row('TCPair','tx','cx','rx'),row('ExtractedExpr','x','rx'),
        row('TCPair','ty','cy','ry'),row('ExtractedExpr','y','ry')],
        'head':[{'op':'let','dst':'sum','call':{'primitive':'+','output':'i64'},'args':[v('cx'),v('cy')]},
                {'op':'let','dst':'new','call':{'primitive':'+','output':'i64'},'args':[{'literal':1},v('sum')]},
                {'op':'let','dst':'tc','call':{'table':'TCPair'},'args':[v('term'),v('new')]}]}]}]}


class AffineTests(unittest.TestCase):
    def test_compile_immutable_coefficients_and_checked_order(self):
        p=compile_program(fixture_core());r=p['recipes'][0]
        self.assertEqual(r['coefficients'],[1,1,0,1]);self.assertEqual(exact(r,[4,6,0],15),(11,4,51))
        self.assertEqual(exact(r,[4,6,0],0,has_old=False),(11,0,37))
        self.assertEqual(exact(r,[4,6,0],15,complete=False),(0,0,0))
        self.assertEqual(exact(r,[HI,1,0],15),(0,0,0))
        self.assertEqual(exact(r,[10**16,2,0],10**16+5),(10**16+3,2,19))
        # Final affine expression fits, but an earlier native instruction fails.
        r=dict(r,operations=[[0,0,0,1,1],[1,0,3,1,1]],result=(0,4),coefficients=[1,0,0,0])
        self.assertEqual(exact(r,[HI,0,0],HI),(0,0,0))

    def test_delta_overflow_does_not_invalidate_candidate(self):
        r={'operations':[],'result':(0,0),'matrix_range_certificate':True}
        new,delta,flags=exact(r,[LO,0,0],HI)
        self.assertEqual(new,LO);self.assertEqual(delta,0);self.assertTrue(flags&1);self.assertTrue(flags&2);self.assertFalse(flags&16);self.assertTrue(flags&8)

    def test_correlated_choices_missing_and_alias_constraints(self):
        eq=lambda s,x:{'sort':s,'eq':x};num=lambda x:{'sort':'i64','value':str(x)}
        row=lambda t,*v:{'table':t,'values':list(v),'subsumed':False}
        x=eq('E','x');y=eq('E','y');root=eq('E','r');rx=eq('TC','rx');ry=eq('TC','ry')
        a=eq('T','a');b=eq('T','b')
        snap={'rows':[row('Bop',x,y,root),row('ExtractedExpr',x,rx),row('ExtractedExpr',y,ry),
                      row('TCPair',a,num(2),rx),row('TCPair',b,num(9),rx),row('TCPair',a,num(3),ry)]}
        p,m,w,_=contexts(snap,[('E','eq','r')],compile_program(fixture_core()))
        self.assertEqual(p[:,:2].tolist(),[[2,3],[9,3]]);self.assertEqual(m[:,2].tolist(),[1,1]);self.assertEqual(m[:,3].tolist(),[0,0])
        self.assertEqual(w[1][1][0],(('T','eq','b'),9))
        # Same term variable across the two witnesses may not mix a and b.
        core=fixture_core();core['rulesets'][0]['rules'][0]['body'][3]['args'][0]['var']='tx'
        p,m,_,_=contexts(snap,[('E','eq','r')],compile_program(core));self.assertEqual(p[:,:2].tolist(),[[2,3]])
        snap['rows']=[r for r in snap['rows'] if not(r['table']=='ExtractedExpr' and r['values'][0]==y)]
        p,m,_,_=contexts(snap,[('E','eq','r')],compile_program(fixture_core()));self.assertEqual(m[:,2].sum(),0)

    @unittest.skipUnless(os.environ.get('POLYSPINE_AFFINE_WORKER'),'Metal worker required')
    def test_gpu_sentinel_overflow_missing_and_fused_score(self):
        program=compile_program(fixture_core());r=program['recipes'][0]
        p=np.array([[4,6,0,15],[10**16,2,0,10**16+5],[HI,1,0,HI],[2048,-2048,0,0],[-2049,0,0,HI],[4,6,0,0],[4,6,0,15]],'<i8')
        m=np.array([[i,0,1,i!=5] for i in range(len(p))],'<u4');m[-1,2]=0;n=len(p);h=np.zeros((n,512),'<f2');h[:,0]=np.arange(n)
        width=520;w=np.zeros((width,8),'<f2');w[0,0]=64;w[512:516,1]=r['coefficients'];w[512:516,2]=-np.array(r['coefficients']);w[516,2]=1
        with tempfile.TemporaryDirectory() as td:
            d=Path(td);(d/'input.json').write_text(json.dumps({'frames':[{'id':0,'packet':'input.bin'}]}))
            with (d/'input.bin').open('wb') as s:
                s.write(b'AFFINE01');s.write(struct.pack('<4I',n,n,width,1))
                for a in [h,p,m,w,np.arange(n+1,dtype='<u4'),np.arange(n,dtype='<u4')]:s.write(a.tobytes())
            (d/'kernel.metal').write_text(Path(__file__).with_name('affine.metal').read_text().replace('// COMPILED_PROGRAM',metal_program(program)))
            subprocess.run([os.environ['POLYSPINE_AFFINE_WORKER'],str(d/'input.json'),str(d/'out'),str(d/'kernel.metal')],check=True)
            expected=np.array([exact(r,c[:3],int(c[3]),bool(row[2]),bool(row[3])) for c,row in zip(p,m)],np.int64)
            actual=np.fromfile(d/'out/frame-0-exact.bin','<i8').reshape(-1,3);np.testing.assert_array_equal(actual,expected)
            matrix=np.fromfile(d/'out/frame-0-matrix.bin','<f4').reshape(-1,3);np.testing.assert_array_equal(matrix[:,0],64*np.arange(n))
            f=np.fromfile(d/'out/frame-0-features.bin','<f4').reshape(n,24);np.testing.assert_allclose(f,reference_features(p,m,expected,n),atol=3e-6,rtol=1e-5)

    @unittest.skipUnless(os.environ.get('POLYSPINE_NATIVE_FRONTEND'),'native frontend required')
    def test_compiled_cost_matches_native_rhs(self):
        source='''(datatype E (A) (B) (Root E E))
(datatype T (Atom) (Pair T T))
(datatype TC (TCPair T i64))
(function ExtractedExpr (E) TC :merge old)
(relation PotentialExtractedExpr (E TC))
(ruleset terms)
(rule ((= root (Root x y)) (= (TCPair tx cx) (ExtractedExpr x)) (= (TCPair ty cy) (ExtractedExpr y)))
      ((PotentialExtractedExpr root (TCPair (Pair tx ty) (+ 1 (+ cx cy))))) :ruleset terms)
(let a (A)) (let b (B)) (Root a b)
(set (ExtractedExpr a) (TCPair (Atom) 4))
(set (ExtractedExpr b) (TCPair (Atom) 6))
(run-schedule (run terms))
'''
        with tempfile.TemporaryDirectory() as td:
            d=Path(td);file=d/'input.egg';file.write_text(source);native=Oracle(Path(os.environ['POLYSPINE_NATIVE_FRONTEND']),file,d)
            try:
                program=compile_program(native.core);want=exact(program['recipes'][0],[4,6,0],15)[0]
                native.ask({'op':'run','set':'terms'});snap=native.ask({'op':'snapshot'})
                costs=[int(r['values'][1]['value']) for r in snap['rows'] if r['table']=='TCPair']
                self.assertIn(want,costs);self.assertEqual(want,11)
            finally:native.close()

if __name__=='__main__':unittest.main()

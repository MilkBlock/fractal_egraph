import copy,json,os,subprocess,tempfile,unittest
from pathlib import Path
import numpy as np
from marker_context import compile_links,export_context,reference_step,reference_features,write_packet
from affine_terms import metal_program,HI
from test_affine import fixture_core
from study import Oracle


def core():
    c=fixture_core();r={'name':'leaf','body':[{'call':{'table':'Const'},'args':[{'var':'root'}]}],
       'head':[{'op':'let','dst':'pair','call':{'table':'TCPair'},'args':[{'var':'term'},{'literal':1}]}]}
    c['rulesets'][0]['rules'].append(r)
    c['rulesets'].append({'name':'terms-helpers','rules':[{'head':[{'call':{'table':'TCPair'},'args':[{'var':'none'},{'literal':10**16}]}]}]})
    return c


def eq(sort,name):return {'sort':sort,'eq':name}
def row(table,*v):return {'table':table,'values':list(v),'subsumed':False}
def num(v):return {'sort':'i64','value':v}


def fixture():
    a,b,c,d=[eq('E',x) for x in 'abcd'];rx,ry,rz=[eq('TC',x) for x in ['rx','ry','rz']];x=eq('E','x')
    rows=[row('Const',a),row('Bop',a,a,b),row('Bop',b,a,c),row('Bop',c,a,d),row('Bop',b,b,d),
          row('TCPair',eq('T','one'),num(7),rx),row('TCPair',eq('T','two'),num(3),ry),row('Smaller',rx,ry,rz),row('ExtractedExpr',x,rz)]
    return {'rows':rows},[('E','eq',k) for k in 'abcdx']


class MarkerTests(unittest.TestCase):
    def test_unresolved_ports_and_all_alternatives_survive_export(self):
        snap,keys=fixture();c=export_context(snap,keys,compile_links(core()))
        self.assertEqual(c['initial'][:4,1].sum(),0)
        self.assertEqual(len(c['index'][c['offsets'][3]:c['offsets'][4]]),2)
        # b depends twice on a: multiplicity is arithmetic, not set union.
        b=next(r for r in c['rows'] if r[0]==1);self.assertEqual(b[2],b[3]);self.assertEqual(b[5],2)
        self.assertGreater(len(c['dependents']),len(c['rows']))
        self.assertEqual(len(c['seeds']),2);self.assertEqual(len(c['bindings']),len(c['rows']))

    def test_multistep_and_pending_comparison(self):
        snap,keys=fixture();p=compile_links(core());c=export_context(snap,keys,p);s=c['initial']
        s,_=reference_step(c,p,s);self.assertTrue(s[0,1]);self.assertFalse(s[1,1]);self.assertFalse(s[4,1])
        s,_=reference_step(c,p,s);self.assertEqual(s[1,0],3);self.assertEqual(s[4,0],3)
        for _ in range(6):s,_=reference_step(c,p,s)
        np.testing.assert_array_equal(s[:5,0],[1,3,5,7,3]);self.assertTrue(np.all(s[:5,1]))

    def test_unfounded_cycle_is_not_an_answer(self):
        a=eq('E','a');c=export_context({'rows':[row('Bop',a,a,a)]},[('E','eq','a')],compile_links(core()));s=c['initial']
        for _ in range(10):s,_=reference_step(c,compile_links(core()),s)
        self.assertEqual(s[0,1],0)

    def test_negative_and_cross_port_alias_are_rejected(self):
        c=core();c['rulesets'][0]['rules'][0]['head'][0]['call']['primitive']='-'
        with self.assertRaises(ValueError):compile_links(c)
        c=core();c['rulesets'][0]['rules'][0]['body'][3]['args'][0]['var']='tx'
        with self.assertRaises(ValueError):compile_links(c)

    def test_checked_overflow_does_not_publish_initializer(self):
        a,b=[eq('E',x) for x in ['a','b']];tc=eq('TC','tc')
        snap={'rows':[row('Bop',a,a,b),row('ExtractedExpr',a,tc),row('TCPair',eq('T','huge'),num(HI),tc)]}
        p=compile_links(core());c=export_context(snap,[('E','eq','b')],p);s,cand=reference_step(c,p,c['initial'])
        self.assertFalse(s[0,1]);self.assertFalse(cand[0,1])

    @unittest.skipUnless(os.environ.get('POLYSPINE_MARKER_WORKER'),'Metal worker required')
    def test_gpu_resumes_from_wiring_without_cpu_parameters(self):
        snap,keys=fixture();p=compile_links(core());c=export_context(snap,keys,p)
        with tempfile.TemporaryDirectory() as td:
            d=Path(td);write_packet(c,p,d/'in.bin');(d/'input.json').write_text(json.dumps({'frames':[{'id':0,'packet':'in.bin'}]}))
            (d/'kernel.metal').write_text(Path(__file__).with_name('marker.metal').read_text().replace('// COMPILED_PROGRAM',metal_program(p)))
            subprocess.run([os.environ['POLYSPINE_MARKER_WORKER'],str(d/'input.json'),str(d/'out'),str(d/'kernel.metal')],check=True,stdout=subprocess.DEVNULL)
            s=c['initial']
            for k in range(1,9):
                old=s;s,can=reference_step(c,p,s)
                if k in [1,2,4,8]:
                    actual=np.fromfile(d/f'out/frame-0-depth-{k}-state.bin','<i8').reshape(-1,4);np.testing.assert_array_equal(actual,s)
                    actual=np.fromfile(d/f'out/frame-0-depth-{k}-features.bin','<f4').reshape(-1,24);np.testing.assert_allclose(actual,reference_features(c,s,can,old),atol=3e-6,rtol=1e-5)

    @unittest.skipUnless(os.environ.get('POLYSPINE_NATIVE_FRONTEND'),'native frontend required')
    def test_multilevel_native_cost_oracle(self):
        source='''(datatype E (Const) (Bop E E))
(datatype T (Leaf) (Node T T))
(datatype TC (TCPair T i64) (Smaller TC TC))
(function ExtractedExpr (E) TC :merge (Smaller old new))
(relation PotentialExtractedExpr (E TC))
(ruleset terms) (ruleset terms-helpers)
(rule ((= e (Const))) ((PotentialExtractedExpr e (TCPair (Leaf) 1))) :ruleset terms)
(rule ((= e (Bop a b)) (= (TCPair ta ca) (ExtractedExpr a)) (= (TCPair tb cb) (ExtractedExpr b)))
 ((PotentialExtractedExpr e (TCPair (Node ta tb) (+ 1 (+ ca cb))))) :ruleset terms)
(rule ((PotentialExtractedExpr e tc)) ((set (ExtractedExpr e) (TCPair (Leaf) 10000000000000000))) :ruleset terms-helpers)
(rule ((PotentialExtractedExpr e (TCPair t c))) ((set (ExtractedExpr e) (TCPair t c))) :ruleset terms-helpers)
(rule ((= s (Smaller (TCPair a x) (TCPair b y))) (<= x y)) ((union s (TCPair a x))) :ruleset terms-helpers)
(rule ((= s (Smaller (TCPair a x) (TCPair b y))) (> x y)) ((union s (TCPair b y))) :ruleset terms-helpers)
(let a (Const)) (let b (Bop a a)) (let c (Bop b a)) (Bop c a)
(run-schedule (saturate (seq (run terms) (run terms-helpers))))
'''
        with tempfile.TemporaryDirectory() as td:
            d=Path(td);file=d/'fixture.egg';file.write_text(source);native=Oracle(Path(os.environ['POLYSPINE_NATIVE_FRONTEND']),file,d)
            try:
                pre=native.ask({'op':'snapshot'});keys=sorted({('E','eq',r['values'][-1]['eq']) for r in pre['rows'] if r['table'] in ['Const','Bop']});p=compile_links(native.core);c=export_context(pre,keys,p);s=c['initial']
                for _ in range(8):s,_=reference_step(c,p,s)
                native.ask({'op':'time_runs','sets':['terms','terms-helpers']*16});post=native.ask({'op':'snapshot'})
                from affine_terms import pair_index
                pairs,_=pair_index(post)
                np.testing.assert_array_equal(s[:len(keys),0],[min(v for t,v in pairs[k]) for k in keys])
            finally:native.close()

    @unittest.skipUnless(os.environ.get('POLYSPINE_NATIVE_FRONTEND'),'native frontend required')
    def test_analysis_capture_automatically_emits_resumable_context(self):
        import contextlib,io
        from scale_capture import capture
        from study import sha
        source="""(datatype Expr (Const) (Bop Expr Expr))
(datatype T (Leaf) (Node T T))
(datatype TC (TCPair T i64) (Smaller TC TC))
(function ExtractedExpr (Expr) TC :merge (Smaller old new))
(relation PotentialExtractedExpr (Expr TC))
(relation HasType (Expr T)) (relation ExprIsPure (Expr)) (relation ExprIsResolved (Expr))
(ruleset always-run) (ruleset is-resolved)
(rule ((= e (Const))) ((ExprIsPure e)) :ruleset always-run)
(rule ((= e (Const))) ((ExprIsResolved e)) :ruleset is-resolved)
(ruleset terms) (ruleset terms-helpers) (ruleset terms-helpers-helpers)
(rule ((= e (Const))) ((PotentialExtractedExpr e (TCPair (Leaf) 1))) :ruleset terms)
(rule ((= e (Bop a b)) (= (TCPair ta ca) (ExtractedExpr a)) (= (TCPair tb cb) (ExtractedExpr b)))
 ((PotentialExtractedExpr e (TCPair (Node ta tb) (+ 1 (+ ca cb))))) :ruleset terms)
(rule ((PotentialExtractedExpr e tc)) ((set (ExtractedExpr e) (TCPair (Leaf) 10000000000000000))) :ruleset terms-helpers)
(rule ((PotentialExtractedExpr e (TCPair t c))) ((set (ExtractedExpr e) (TCPair t c))) :ruleset terms-helpers)
(rule ((= s (Smaller (TCPair a x) (TCPair b y))) (<= x y)) ((union s (TCPair a x))) :ruleset terms-helpers)
(rule ((= s (Smaller (TCPair a x) (TCPair b y))) (> x y)) ((union s (TCPair b y))) :ruleset terms-helpers)
(let a (Const)) (let b (Bop a a)) (Bop b a)
(run-schedule (run always-run) (run is-resolved) (saturate (seq (run terms) (run terms-helpers) (run terms-helpers-helpers))))
"""
        with tempfile.TemporaryDirectory() as td:
            d=Path(td);file=d/'fixture.egg';file.write_text(source);frontend=Path(os.environ['POLYSPINE_NATIVE_FRONTEND']);native=Oracle(frontend,file,d);trace=[]
            try:
                prefix=native.ask({'op':'time_runs','sets':['always-run','is-resolved']})
                trace.extend({'set':name,'updated':s['updated']} for name,s in zip(['always-run','is-resolved'],prefix['steps']))
                for _ in range(30):
                    reply=native.ask({'op':'time_runs','sets':['terms','terms-helpers','terms-helpers-helpers']})
                    trace.extend({'set':name,'updated':s['updated']} for name,s in zip(['terms','terms-helpers','terms-helpers-helpers'],reply['steps']))
                    if not any(s['updated'] for s in reply['steps']):break
                else:self.fail('fixture did not saturate')
                workload={'core':native.core,'native_trace':trace,'source':str(file),'source_sha256':sha(file)}
            finally:native.close()
            wp=d/'workload.json';wp.write_text(json.dumps(workload))
            with contextlib.redirect_stdout(io.StringIO()):result=capture(wp,frontend,d/'analysis')
            self.assertEqual(result['marker_context']['frames'],1)
            manifest=json.loads((d/'analysis/marker_context/input.json').read_text());self.assertEqual(manifest['frames'][0]['rows'],3)
            z=np.load(d/'analysis/marker_context/frame-0.npz');self.assertEqual(z['initial'][:,1].sum(),0)
            self.assertGreater(len(z['dependents']),0)

if __name__=='__main__':unittest.main()

"""Independent exact encoding and native-policy/source execution regressions."""
from fractions import Fraction
import json
import unittest
from .structural_keys import B,label


def encode(key):
    anchor,name,children=key
    cs=[encode(x) for x in children]
    h=0 if anchor else max((x[2]+1 for x in cs),default=0)
    tokens=[1] if anchor else [2]+[257]*h+[1]+label(name)+[0]
    c=Fraction(0);s=Fraction(1)
    for d in tokens:s/=B;c+=d*s
    for cc,ss,_,_ in cs:c+=s*cc;s*=ss
    s/=B # final children-list terminator 0
    return c,s,h,int(anchor)


def order(k):
    a,n,cs=k
    height=0 if a else max((order(c)[1]+1 for c in cs),default=0)
    return (not a,height,n,tuple(order(x) for x in cs))


def verify_dump(folder):
    from collections import Counter
    from pathlib import Path
    folder=Path(folder)
    keys=json.loads((folder/'reference-keys.json').read_text())
    ref={(k['sort'],k['value']):encode(k['key']) for k in keys}
    reference=json.loads((folder/'reference.json').read_text());portable=json.loads((folder/'portable.json').read_text())
    actual={}
    for op,args,value,_ in portable['rows']:
        if op.startswith('CellStructuralKey-'):
            w=json.loads(value[1]['base']);actual[(args[0][0],args[0][1]['eq'])]=(Fraction(w[0]),Fraction(w[1]),int(Fraction(w[2])),int(Fraction(w[3])))
    def lift(g,m,ops):
        def v(x):return (x[0],m[x[0],x[1]['eq']]) if 'eq' in x[1] else (x[0],x[1]['base'])
        return Counter((n,tuple(v(x) for x in xs),v(y),hidden) for n,xs,y,hidden in g['rows'] if n in ops)
    a=lift(reference,ref,reference['tables']);b=lift(portable,actual,reference['tables'])
    if a!=b:raise AssertionError('key-associated original graphs differ: '+str(list((a-b).elements())[:2]))
    if Counter((s,k) for (s,_),k in ref.items())!=Counter((s,k) for (s,_),k in actual.items()):raise AssertionError('key populations differ')
    return dict(reference_key_rows=len(ref),portable_key_rows=len(actual),exact_key_association=True,
        original_rows=sum(a.values()),max_key_bits=max((k[0].denominator.bit_length() for k in actual.values()),default=0))


class KeyEncoding(unittest.TestCase):
    def test_structural_order_matches_rational_order(self):
        leaves=[[False,n,[]] for n in ['i64:-9223372036854775808','i64:-2','i64:0','i64:10','i64:2','String:"a"','String:"\\n"','f64:0000000000000000','f64:8000000000000001','bool:false','bool:true']]
        keys=leaves+[[False,n,[a,b]] for n in ['Expr:Add','Expr:Mul'] for a in leaves for b in leaves]
        keys += [[True,'\0input-anchor',[x]] for x in keys[:30]]
        self.assertEqual(sorted(keys,key=order),sorted(keys,key=lambda k:encode(k)[0]))
        self.assertEqual(len({tuple(encode(k)) for k in keys}),len(keys))
    def test_concat_is_associative(self):
        combine=lambda a,b:(a[0]+a[1]*b[0],a[1]*b[1])
        a,b,c=[encode([False,n,[]])[:2] for n in ['A','B','C']]
        self.assertEqual(combine(combine(a,b),c),combine(a,combine(b,c)))


class StandaloneSource(unittest.TestCase):
    def setUp(self):
        from pathlib import Path
        import tempfile
        from .compiler import ROOT
        self.root=ROOT
        self.tmp=tempfile.TemporaryDirectory(dir=ROOT/'out',prefix='standalone-key-test-')
        self.addCleanup(self.tmp.cleanup);self.out=Path(self.tmp.name)

    def compile(self,source):
        from .structural_keys import compile_source
        return compile_source(source,self.out/'sk')

    def test_source_matches_reused_native_policy_for_all_key_fields(self):
        import subprocess
        for fixture in ['hoist','literals','cycles']:
            with self.subTest(fixture=fixture):
                from .structural_keys import compile_source
                folder=self.out/fixture;folder.mkdir()
                source=self.root/f'experiments/standalone_structural_keys/{fixture}.egg'
                compile_source(source,folder/'sk')
                subprocess.run([self.root/'target/analyzer/release/examples/standalone_key_verify',source,folder/'sk/source_sk.egg',folder/'verification'],check=True,capture_output=True)
                self.assertTrue(verify_dump(folder/'verification')['exact_key_association'])
                text=(folder/'sk/source_sk.egg').read_text()
                for custom in ['cell-key-min','cell-key-cmp','cell-key-decode','cell-hoist-best']:
                    self.assertNotIn(custom,text)

    def test_computed_string_fails_instead_of_silently_missing_a_key(self):
        import subprocess
        src=self.out/'input.egg'
        src.write_text('(datatype E (S String)) (ruleset r) (rule ((= e (S s))) ((S (+ s "x"))) :ruleset r) (S "a") (run r 1)')
        with self.assertRaises(subprocess.CalledProcessError):self.compile(src)

    def test_wrong_eggcc_selection_schema_is_not_guessed(self):
        src=self.out/'input.egg'
        src.write_text('(datatype Expr (V i64)) (function to-hoist (Expr) Expr :merge new) (ruleset r) (V 1) (run r 1)')
        with self.assertRaisesRegex(ValueError,'eggcc schema'):self.compile(src)

    def test_unused_container_is_allowed_and_instantiation_fails(self):
        import subprocess
        from .structural_keys import compile_source
        base='(datatype E (V i64)) (sort Es (Vec E)) (constructor Pack (Es) E) (ruleset r) '
        for active in [False,True]:
            with self.subTest(active=active):
                src=self.out/f'{active}.egg'
                src.write_text(base+('(Pack (vec-of (V 1))) ' if active else '(V 1) ')+'(run r 1)')
                dest=self.out/str(active);compile_source(src,dest)
                run=subprocess.run([self.root/'target/release/examples/native_math_baseline',dest/'source_sk.egg'],capture_output=True,text=True)
                self.assertEqual(run.returncode==0,not active,run.stderr)
                if active:self.assertIn('unsupported structural key field',run.stderr)

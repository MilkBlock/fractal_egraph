"""Compare function-root DAGs; optionally prove equality by Concat associativity.

This is not a general semantic equivalence checker or a quality cost estimate.
The original exact whole-DAG Tiger check is retained separately.
"""
import argparse,json,subprocess
from pathlib import Path

class Intern:
    def __init__(self):self.ids={};self.nodes=[]
    def add(self,key):
        if key not in self.ids:self.ids[key]=len(self.nodes);self.nodes.append(key)
        return self.ids[key]

def roots(program,interner,associate):
    tables={t['name']:t for t in program['tables']};nodes={};functions={}
    for row in program['initial']:
        t=tables[row['table']]
        if t['kind']!='constructor':continue
        if row['subsumed']:raise ValueError('selected DAG contains hidden constructors')
        vs=row['values'];key=(vs[-1]['sort'],vs[-1]['eq'])
        if key in nodes:raise ValueError('selected e-class has multiple constructors')
        nodes[key]=(t['name'],vs[:-1])
        if t['name']=='Function':
            name=vs[0]['value']
            if name in functions:raise ValueError('duplicate function root')
            functions[name]=key
    memo={};active=set()
    def scalar(x):
        if 'eq' in x:return memo[(x['sort'],x['eq'])]
        if 'elements' in x:raise ValueError('selected container normalization unsupported')
        return interner.add(('literal',x['sort'],x['value']))
    for root in functions.values():
        stack=[(root,False)]
        while stack:
            key,done=stack.pop()
            if key in memo:continue
            if key not in nodes:raise ValueError('selected class has no constructor')
            op,vs=nodes[key]
            if not done:
                if key in active:raise ValueError('selected DAG has a cycle')
                active.add(key);stack.append((key,True))
                for v in reversed(vs):
                    if 'eq' in v:stack.append(((v['sort'],v['eq']),False))
                continue
            children=tuple(scalar(v) for v in vs)
            if associate and op=='Concat':
                if key[0]!='Expr' or len(vs)!=2 or any(v['sort']!='Expr' for v in vs):raise ValueError('unexpected Concat schema')
                flat=[]
                for c in children:
                    n=interner.nodes[c]
                    flat.extend(n[1] if n[0]=='concat-sequence' else [c])
                memo[key]=interner.add(('concat-sequence',tuple(flat)))
            else:memo[key]=interner.add(('node',key[0],op,children))
            active.remove(key)
    return {name:memo[k] for name,k in functions.items()},len(memo)

def compare_programs(a,b):
    result={}
    for associate,label in [(False,'exact_roots'),(True,'concat_associative_roots')]:
        intern=Intern();ra,na=roots(a,intern,associate);rb,nb=roots(b,intern,associate)
        result[label]={'equal':ra==rb,'different_functions':sorted(k for k in ra.keys()|rb.keys() if ra.get(k)!=rb.get(k)),'function_count':[len(ra),len(rb)],'reachable_classes':[na,nb]}
    result['scope']='Function-root DAG comparison. The only added equation is Concat(Concat(a,b),c)=Concat(a,Concat(b,c)); no empirical semantic or cost claim.'
    return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--native',type=Path,required=True);p.add_argument('--gpu',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    programs=[]
    for path in [a.native,a.gpu]:programs.append(json.loads(subprocess.check_output([str(a.build.resolve()/'frontend'),str(path.resolve()),'--selected-dag'],text=True)))
    result=compare_programs(*programs);a.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))

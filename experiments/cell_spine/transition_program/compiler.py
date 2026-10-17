"""Compile a supplied finite boundary schedule, retaining both parents' facts.

No discovery of external adjacency, general effect lowering, or global union
engine. Native Bake and rule parsing precede the binding-independent programs.
"""
from itertools import product
import json
from pathlib import Path

from experiments.cell_spine.factorized_boundary.demo import Graph, native, sexpr
from experiments.cell_spine.moment_compiler.compiler import Circuit, certify_commutative


def freeze(x):
    return tuple(map(freeze,x)) if isinstance(x,(list,tuple)) else x


def ast_term(x):
    if x['kind']=='var':return ('V',x['name'])
    if x['kind']!='call':raise ValueError('this compiler requires constructor-only rewrites')
    return (x['head'],*(ast_term(c) for c in x['args']))


def variables(t):
    if t[0]=='V':return [t[1]]
    return list(dict.fromkeys(v for c in t[1:] for v in variables(c)))


def minimum_terms(graph):
    """Grounded minimum representatives, also for cyclic saturated classes."""
    result={}
    while True:
        changed=False
        for row in graph.rows:
            if row['op']=='RipenInput':
                term=('Input',int(row['args'][0][1]))
            elif row['op']=='Var':
                term=('Var',row['args'][0][1])
            elif all(v in result for s,v in row['args'] if s=='Math'):
                if any(s!='Math' for s,_ in row['args']):continue
                term=(row['op'],*(result[v] for _,v in row['args']))
            else:continue
            old=result.get(row['result'])
            if old is None or (len(sexpr(term)),sexpr(term))<(len(sexpr(old)),sexpr(old)):
                result[row['result']]=term;changed=True
        if not changed:break
    return result


def side_bindings(pattern, graph, root):
    """Offline partial-pattern matching against a symbolic baked Cell."""
    def visit(p,value):
        if p[0]=='V':return [{p[1]:value}]
        found=[]
        for row in graph.by_class[value]:
            if row['op']!=p[0] or len(row['args'])!=len(p)-1:continue
            for pieces in product(*(visit(c,v) for c,(_,v) in zip(p[1:],row['args']))):
                merged={};valid=True
                for piece in pieces:
                    for name,v in piece.items():
                        if name in merged and merged[name]!=v:valid=False
                        merged[name]=v
                if valid:found.append(merged)
        return found
    reps=minimum_terms(graph)
    unique={tuple(sorted(b.items())) for b in visit(pattern,root)}
    return [{name:reps[value] for name,value in b} for b in sorted(unique)]


def scoped(t,instance):
    if t[0]=='Input':return ('Ref',instance,t[1])
    return (t[0],*(scoped(c,instance) for c in t[1:]))


def normalize_add(t):
    if t[0] in ('Ref','Input','Var','V'):return t
    children=[normalize_add(c) for c in t[1:]]
    return (t[0],*(sorted(children) if t[0]=='Add' else children))


def substitute_tree(t,mapping):
    key=normalize_add(t)
    if key in mapping:return mapping[key]
    if t[0]=='Ref':return t
    return (t[0],*(substitute_tree(c,mapping) for c in t[1:]))


def parameterize(seed,rhs):
    slots=[]
    def visit(t,allow_new):
        if t[0]=='Ref':
            ref=t[1:]
            if ref not in slots:
                if not allow_new:raise ValueError('RHS input absent from retained context')
                slots.append(ref)
            return ('Input',slots.index(ref))
        return (t[0],*(visit(c,allow_new) for c in t[1:]))
    initial=visit(seed,True);output=visit(rhs,False)
    return initial,output,slots


class Bake:
    def __init__(self,out,binary,source):
        self.out,self.binary,self.source=out,binary,source
        self.templates=[];self.index={};self.graphs=[]

    def add(self,seed,rhs=None):
        seed=freeze(seed)
        if seed not in self.index:
            tid=len(self.templates);self.index[seed]=tid
            directory=self.out/f't{tid}'
            env=self.source.replace('(Var String)','(RipenInput i64)')
            check='' if rhs is None else f'(check (= root {sexpr(rhs)}))'
            native(self.binary,directory,env,f'(let root {sexpr(seed)})',check,
                   ['RipenInput','Add','Mul','Pow','Sin','Cos','Diff','Integral'])
            graph=Graph(directory/'graph.json');self.graphs.append(graph)
            inputs=[]
            def collect(t):
                if t[0]=='Input':inputs.append(t[1])
                else:
                    for c in t[1:]:collect(c)
            collect(seed)
            arity=max(inputs)+1
            self.templates.append(dict(seed=seed,arity=arity,root=graph.value(seed),
                enodes=len(graph.rows),eclasses=len(graph.by_class),rhs_checks=[]))
        tid=self.index[seed]
        if rhs is not None:
            # Native saturation must have introduced and unioned the exact RHS.
            graph=self.graphs[tid]
            assert graph.value(rhs)==self.templates[tid]['root']
            self.templates[tid]['rhs_checks'].append(rhs)
        return tid

    def bindings(self,tid,pattern):
        return side_bindings(pattern,self.graphs[tid],self.templates[tid]['root'])


class Program:
    def __init__(self,bake,ast,bits=4):
        self.bake,self.ast,self.bits=bake,ast,bits
        self.instances=[];self.branches=[];self.stages=[]
        self.rules={}
        selected={('Add','Mul'):'factor',('Mul','Pow'):'powers',('Diff','Cos'):'diff-sin',('Integral','Sin'):'integral-cos'}
        for r in ast['rewrites']:
            lhs,rhs=ast_term(r['lhs']),ast_term(r['rhs'])
            name=selected.get((lhs[0],rhs[0]))
            if name:
                if r['conditions'] or r['subsume'] or r['bidirectional']:
                    raise ValueError('only unconditional positive rewrites')
                self.rules[name]=dict(lhs=lhs,rhs=rhs,variables=variables(lhs),source=r['source']['text'])

    def instance(self,name,tid,binding=None):
        i=len(self.instances)
        self.instances.append(dict(name=name,template=tid,binding=binding,initial=binding is not None))
        return i

    def edge(self,label,rule,left,right):
        r=self.rules[rule];lhs=r['lhs']
        common=set(variables(lhs[1])) & set(variables(lhs[2]))
        if not common:raise ValueError('fixture expects a shared-variable boundary')
        choices=[self.bake.bindings(self.instances[i]['template'],p) for i,p in zip((left,right),lhs[1:])]
        found=[]
        for lb,rb in product(*choices):
            left_bind={v:scoped(t,left) for v,t in lb.items()}
            right_bind={v:scoped(t,right) for v,t in rb.items()}
            conditions=[(left_bind[v],right_bind[v]) for v in sorted(common)]
            # These fixtures admit only Input/Add common interfaces. Incompatible
            # shapes cannot match under this guarded, finite library.
            def shape(t):
                if t[0]=='Ref':return 'I'
                if t[0]!='Add':raise ValueError('no certified codec for '+t[0])
                return 'A('+','.join(sorted(shape(c) for c in t[1:]))+')'
            shapes=[(shape(a),shape(b)) for a,b in conditions]
            if any(a!=b for a,b in shapes):continue
            seed_l=scoped(self.bake.templates[self.instances[left]['template']]['seed'],left)
            seed_r=scoped(self.bake.templates[self.instances[right]['template']]['seed'],right)
            replacements={normalize_add(right_bind[v]):left_bind[v] for v in common}
            # Equality substitution retains the entire right-parent seed,
            # including facts that cannot be recovered from a one-way RHS.
            retained=(lhs[0],seed_l,substitute_tree(seed_r,replacements))
            bindings=dict(right_bind,**left_bind)
            def rhs(t):
                return bindings[t[1]] if t[0]=='V' else (t[0],*(rhs(c) for c in t[1:]))
            seed,output,slots=parameterize(retained,rhs(r['rhs']))
            tid=self.bake.add(seed,output)
            target=self.instance(f'{label}/{len(found)}',tid)
            branch=dict(label=label,rule=rule,left=left,right=right,target=target,
                        conditions=conditions,shared_shapes=[a for a,_ in shapes],
                        bindings=slots,partition=list(range(len(slots))),rhs=output,binding_terms=bindings)
            self.branches.append(branch);found.append(len(self.branches)-1)
        return found

    def pick(self,branches,shape='A(I,I)'):
        selected=[i for i in branches if self.branches[i]['shared_shapes']==[shape]]
        if len(selected)!=1:raise ValueError('ambiguous example continuation')
        return self.branches[selected[0]]['target']

    def lower(self):
        slots=max(t['arity'] for t in self.bake.templates)
        c=Circuit(self.bits,slots,certify_commutative(self.ast,'Add'))
        jobs=[]
        for stage in self.stages:
            groups={};requirements={}
            for bi in stage:
                branch=self.branches[bi];pairs=[]
                for a,b in branch['conditions']:
                    def compile_ref(t):
                        instances=set()
                        def local(n):
                            if n[0]=='Ref':instances.add(n[1]);return ('Input',n[2])
                            return (n[0],*(local(x) for x in n[1:]))
                        term=local(t)
                        if len(instances)!=1:raise ValueError('an interface belongs to one source Cell')
                        shape,node=c.compile(term)
                        return shape,(next(iter(instances)),node)
                    sa,wa=compile_ref(a);sb,wb=compile_ref(b);assert sa==sb
                    groups.setdefault(sa,set()).update((wa,wb));pairs.append((sa,wa,wb))
                requirements[bi]=pairs
            blocks=[];queries=[];where={}
            for shape,wireset in sorted(groups.items()):
                wires=sorted(wireset);positions={w:i for i,w in enumerate(wires)}
                width=c.nodes[wires[0][1]]['width']
                bound=max(width*c.nodes[n]['bound']**2 for _,n in wires)
                if 4*bound>=1<<24:raise ValueError('FP32 exactness bound')
                i=len(blocks);blocks.append(dict(width=width,wires=wires,max_norm=bound))
                queries.append(dict(shape=shape,wires=wires,terms=[dict(block=i,rows=list(range(len(wires))))]))
                where[shape]=(i,positions,len(wires))
            transitions=[]
            for bi in stage:
                b=self.branches[bi];conditions=[]
                for shape,left,right in requirements[bi]:
                    qi,positions,n=where[shape];conditions.append([qi,positions[left]*n+positions[right]])
                transitions.append(dict(branch=bi,target=b['target'],sources=[b['left'],b['right']],
                    conditions=conditions,bindings=b['bindings'],partition=b['partition']))
            guards=[list(range(self.bake.templates[i['template']]['arity'])) for i in self.instances]
            jobs.append(dict(name=f'stage-{len(jobs)}',plan=dict(blocks=blocks,queries=queries,guards=guards),
                             transitions=transitions))
        return c,jobs

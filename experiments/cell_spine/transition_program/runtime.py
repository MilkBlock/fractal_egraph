"""Adaptive local Cell runtime over supplied constructor facts.

CPU owns Bake, exact interning, and worklist routing. The optional GPU backend
decides matches and constructs target coordinates. This is not global egglog.
"""
from collections import Counter,defaultdict,deque
from copy import deepcopy
import json
from pathlib import Path
import subprocess

from .compiler import Bake,Program,freeze,minimum_terms,scoped
from .demo import reference,decode
from experiments.cell_spine.factorized_boundary.demo import Graph,native,sexpr
from experiments.cell_spine.moment_compiler.compiler import bit_code


def validate_environment(ast):
    """Compatibility entry point; transport-proof checks live with the cache."""
    from .specializations import validate_environment as validate
    return validate(ast)


def partition(values):
    unique=[];indices=[];positions={}
    for v in values:
        if v not in positions:positions[v]=len(unique);unique.append(v)
        indices.append(positions[v])
    return tuple(indices),tuple(unique)


def remap(t,mapping):
    if t[0]=='Input':return ('Input',mapping[t[1]])
    return (t[0],*(remap(c,mapping) for c in t[1:]))


def closure_key(graph,root,labels=None):
    """Exact rooted whole-state key, including cycles and retained old facts.

    Ground minimum terms name classes uniquely; parameter permutation is handled
    by recomputing these names with actual input identities, not string renaming.
    """
    reps=minimum_terms(graph,labels)
    if len(reps)!=len(graph.by_class) or len(set(reps.values()))!=len(reps):
        raise ValueError('closure is ungrounded or input aliases were not specialized')
    rows=[]
    for r in graph.rows:
        if r['op']=='RipenInput':
            slot=int(r['args'][0][1])
            rows.append(('Input' if labels is None else 'Var',
                         (slot if labels is None else labels[slot],),reps[r['result']]))
        else:
            args=tuple(reps[v] if sort=='Math' else (sort,v) for sort,v in r['args'])
            rows.append((r['op'],args,reps[r['result']]))
    return reps[root],tuple(sorted(rows))


class Library(Bake):
    def __init__(self,*args,constructors=None):
        super().__init__(*args)
        self.constructors=constructors or ['RipenInput','Add','Mul','Pow','Sin','Cos','Diff','Integral']
        self.closed={};self.aliases={};self.projections={};self.stats=Counter()

    def add(self,seed,rhs=None):
        seed=freeze(seed)
        if seed in self.index:
            tid=self.index[seed];self.stats['seed_hits']+=1
        else:
            attempt=self.stats['native_bakes'];self.stats['native_bakes']+=1
            directory=self.out/f'bake-{attempt}'
            native(self.binary,directory,self.source.replace('(Var String)','(RipenInput i64)'),
                   f'(let root {sexpr(seed)})','',
                   self.constructors)
            graph=Graph(directory/'graph.json');root=graph.value(seed)
            key=closure_key(graph,root)
            if key in self.closed:
                tid=self.closed[key];self.stats['whole_closure_template_hits']+=1
            else:
                tid=len(self.templates);self.closed[key]=tid
                arity=1+max(int(r['args'][0][1]) for r in graph.rows if r['op']=='RipenInput')
                self.graphs.append(graph)
                self.templates.append(dict(seed=seed,root=root,arity=arity,enodes=len(graph.rows),
                                           eclasses=len(graph.by_class),rhs_checks=[]))
            self.index[seed]=tid
        if rhs is not None:
            assert self.graphs[tid].value(rhs)==self.templates[tid]['root']
        return tid

    def specialize(self,tid,values,rhs=None):
        pattern,unique=partition(values)
        if pattern==tuple(range(len(values))):
            if rhs is not None:assert self.graphs[tid].value(rhs)==self.templates[tid]['root']
            return tid,unique
        key=(tid,pattern)
        if key not in self.aliases:
            target=self.add(remap(self.templates[tid]['seed'],pattern))
            self.aliases[key]=target;self.stats['alias_variants_baked']+=1
        else:self.stats['alias_cache_hits']+=1
        target=self.aliases[key]
        if rhs is not None:
            assert self.graphs[target].value(remap(rhs,pattern))==self.templates[target]['root']
        return target,unique

    def bindings(self,tid,pattern):
        key=(tid,freeze(pattern))
        if key not in self.projections:self.projections[key]=super().bindings(tid,pattern)
        else:self.stats['projection_cache_hits']+=1
        return self.projections[key]


class Cells:
    def __init__(self,library):
        self.library=library;self.parent={};self.records=[];self.keys={}
        self.by_instance={};self.descriptor_uses=defaultdict(set);self.uses=defaultdict(set);self.stats=Counter()

    def find_input(self,x):
        self.parent.setdefault(x,x)
        if self.parent[x]!=x:self.parent[x]=self.find_input(self.parent[x])
        return self.parent[x]

    def resolve(self,h):
        row=self.records[h]
        if 'redirect' in row:
            row['redirect']=self.resolve(row['redirect']);return row['redirect']
        return h

    def get(self,h):return self.records[self.resolve(h)]

    def intern(self,tid,values,rhs=None):
        if len(values)!=self.library.templates[tid]['arity']:raise ValueError('wrong Cell arity')
        values=tuple(self.find_input(v) for v in values)
        tid,values=self.library.specialize(tid,values,rhs)
        descriptor=(tid,values)
        if descriptor in self.by_instance:
            self.stats['descriptor_hits']+=1
            return self.resolve(self.by_instance[descriptor])
        t=self.library.templates[tid]
        key=closure_key(self.library.graphs[tid],t['root'],values)
        self.stats['ground_closure_keys']+=1
        if key in self.keys:
            h=self.keys[key];self.stats['exact_instance_hits']+=1
        else:
            h=len(self.records);self.keys[key]=h
            self.records.append(dict(template=tid,binding=values,key=key))
            for v in values:self.uses[v].add(h)
        self.by_instance[descriptor]=h
        self.descriptor_uses[h].add(descriptor)
        return h

    def union_inputs(self,a,b):
        a,b=sorted((self.find_input(a),self.find_input(b)))
        if a==b:return set()
        self.parent[b]=a
        affected=sorted(self.uses[b]);saved=[(h,dict(self.records[h])) for h in affected]
        for h,row in saved:
            if self.keys.get(row['key'])==h:del self.keys[row['key']]
            for v in row['binding']:self.uses[v].discard(h)
        dead=set(affected)
        for h in affected:
            for descriptor in self.descriptor_uses.pop(h,set()):self.by_instance.pop(descriptor,None)
        for h,row in saved:
            target=self.intern(row['template'],row['binding'])
            assert target!=h
            self.records[h]={'redirect':target}
        self.stats['input_unions']+=1;self.stats['cells_recanonicalized']+=len(affected)
        return dead


class Runtime:
    def __init__(self,output,binary,source,ast,backend='cpu',gpu_binary=None,specialize_facts=True):
        self.output=Path(output);self.output.mkdir(parents=True,exist_ok=False)
        constructors=['RipenInput']+[name for name,sig in ast['signatures'].items()
            if sig.get('kind')=='constructor' and sig.get('output')=='Math' and name!='Var']
        self.library=Library(self.output/'library',binary,source,constructors=constructors)
        self.cells=Cells(self.library);self.ast=ast;self.backend=backend;self.gpu_binary=gpu_binary
        self.nodes=[];self.edges={};self.outputs={};self.signatures={};self.edge_cache={}
        self.cache_uses=defaultdict(set)
        self.dependents=defaultdict(set);self.owners=defaultdict(set);self.cell_watch=defaultdict(set)
        self.queue=deque();self.queued=set();self.stats=Counter();self.batches=[];self.events=[]
        self.origins={};self.input_unions=[]
        catalog=Program(self.library,ast,rule_mode='boundary')
        self.rules=catalog.rules;self.bake_only=catalog.bake_only
        self.by_head=defaultdict(list)
        for name,r in self.rules.items():self.by_head[r['lhs'][0]].append(name)
        # Local import avoids a module cycle: the proof helper reuses the exact
        # closure key and equality-partition operations above.
        from .specializations import FactSpecializations
        self.specializations=FactSpecializations(self.library,self.cells,self.context,self.ast) if specialize_facts else None

    def seed(self,term,values):
        if any(not isinstance(v,int) or v<0 for v in values):raise ValueError('input IDs must be nonnegative integers')
        tid=self.library.add(term)
        ref=('cell',self.cells.intern(tid,values))
        self.origins.setdefault(ref,(freeze(term),tuple(values)))
        return ref

    def resolve(self,ref):
        kind,index=ref
        h=index if kind=='cell' else self.outputs.get(index)
        return None if h is None else self.cells.resolve(h)

    def enqueue(self,n):
        if n not in self.queued:self.queue.append(n);self.queued.add(n)

    def construct(self,op,left,right,label=''):
        left,right=tuple(left),tuple(right)
        for kind,index in (left,right):
            limit=len(self.cells.records) if kind=='cell' else len(self.nodes) if kind=='node' else 0
            if not isinstance(index,int) or not 0<=index<limit:raise ValueError('constructor references must already exist')
        key=(op,left,right)
        if key in self.edges:
            self.stats['constructor_fact_hits']+=1;return ('node',self.edges[key])
        n=len(self.nodes);self.edges[key]=n
        self.nodes.append(dict(op=op,left=left,right=right,label=label or f'node-{n}',watched=set()))
        for r in (left,right):self.dependents[r].add(n)
        self.enqueue(n)
        return ('node',n)

    def publish(self,n,h):
        h=self.cells.resolve(h);old=self.outputs.get(n)
        if old==h:return
        if old is not None:self.owners[old].discard(n)
        self.outputs[n]=h;self.owners[h].add(n)
        for parent in self.dependents['node',n]:self.enqueue(parent)

    def context(self,op,left,right):
        slots=[]
        def term(t,values):
            if t[0]=='Input':
                value=values[t[1]]
                if value not in slots:slots.append(value)
                return ('Input',slots.index(value))
            return (t[0],*(term(c,values) for c in t[1:]))
        children=[]
        for h in (left,right):
            cell=self.cells.get(h)
            children.append(term(self.library.templates[cell['template']]['seed'],cell['binding']))
        return (op,*children),tuple(slots)

    def fallback(self,op,left,right):
        seed,values=self.context(op,left,right)
        self.stats['native_context_fallbacks']+=1
        return self.cells.intern(self.library.add(seed),values)

    def union_inputs(self,a,b):
        changed=self.cells.union_inputs(a,b)
        self.input_unions.append((a,b))
        for h in changed:
            for signature in list(self.cache_uses[h]):
                self.edge_cache.pop(signature,None)
                for endpoint in signature[1:]:self.cache_uses[endpoint].discard(signature)
            for n in list(self.cell_watch[h]):self.enqueue(n)
            for n in list(self.owners[h]):self.publish(n,self.cells.resolve(h))
        self.events.append(dict(kind='input_union',left=a,right=b,affected_cells=len(changed)))

    def drain(self):
        while self.queue:
            ready=[]
            current=list(self.queue);self.queue.clear();self.queued.clear()
            for n in current:
                node=self.nodes[n];left,right=self.resolve(node['left']),self.resolve(node['right'])
                if left is None or right is None:continue
                signature=(node['op'],left,right)
                if self.signatures.get(n)==signature:continue
                for h in node['watched']:self.cell_watch[h].discard(n)
                node['watched']={left,right}
                for h in node['watched']:self.cell_watch[h].add(n)
                if signature in self.edge_cache:
                    self.stats['transition_cache_hits']+=1
                    self.signatures[n]=signature
                    self.publish(n,self.edge_cache[signature]);continue
                if self.specializations is not None:
                    h=self.specializations.apply(*signature)
                    if h is not None:
                        self.stats['fact_specialization_hits']+=1
                        self.remember(n,left,right,h,learn=False);continue
                ready.append((n,left,right))
            if not ready:continue
            try:self.process(ready)
            except Exception:
                for n,_,_ in ready:self.enqueue(n)
                raise
        return dict(self.outputs)

    def process(self,ready):
        source_handles=sorted({h for _,l,r in ready for h in (l,r)})
        maximum=max(v for h in source_handles for v in self.cells.get(h)['binding'])
        bits=max(1,maximum.bit_length())
        p=Program(self.library,self.ast,bits,rule_mode='boundary');source_map={}
        for h in source_handles:
            row=self.cells.get(h)
            source_map[h]=p.instance(f'cell-{h}',row['template'],list(row['binding']))
        stage=[];branch_node={};fallback=set();unrepresented=[]
        for n,left,right in ready:
            start_b,start_i=len(p.branches),len(p.instances)
            candidates=[]
            try:
                for rule in self.by_head[self.nodes[n]['op']]:
                    candidates+=p.edge(str(n),rule,source_map[left],source_map[right])
            except ValueError as error:
                p.branches=p.branches[:start_b];p.instances=p.instances[:start_i]
                fallback.add(n);unrepresented.append(dict(node=n,reason=str(error)));continue
            if not candidates:fallback.add(n)
            for bi in candidates:branch_node[bi]=n
            stage+=candidates
        accepted=defaultdict(set);pending=0;counts=Counter()
        if stage:
            p.stages=[stage]
            try:c,jobs=p.lower()
            except ValueError as error:
                for n,left,right in ready:
                    h=self.fallback(self.nodes[n]['op'],left,right)
                    self.remember(n,left,right,h)
                self.batches.append(dict(source_rows=len(source_handles),temporary_rows=0,bits=bits,slots=0,
                    matrices=0,branches=0,ready_nodes=[n for n,_,_ in ready],statuses={},aliases_resumed=0,
                    unsupported=[dict(reason=str(error),stage='matrix compilation')]))
                return
            ports=[];active=[]
            for row in p.instances:
                values=row['binding'] or []
                ports.append([bit_code(v,bits) for v in values]+[bit_code(0,bits)]*(c.slots-len(values)))
                active.append(int(row['initial']))
            half=all(c.nodes[n]['bound']<=2048 for b in jobs[0]['plan']['blocks'] for _,n in b['wires'])
            payload=dict(circuit=c.serialize(),ports=ports,active=active,jobs=jobs,skip_inactive=True,half_inputs=half,chain_stages=False)
            if self.backend=='gpu':
                if self.gpu_binary is None:raise ValueError('GPU executable is required')
                stem=self.output/f'batch-{len(self.batches)}'
                path=stem.with_suffix('.json');result=stem.with_suffix('.result.json')
                path.write_text(json.dumps(payload)+'\n')
                subprocess.run([self.gpu_binary,path,result],check=True,capture_output=True,text=True)
                state=json.loads(result.read_text())['results'][0]
            elif self.backend=='cpu':state=reference(c,jobs,ports,active,skip_inactive=True)[0]
            else:raise ValueError('unknown backend')
            data=state['cell_values']
            values=decode([[data[(i*c.slots+j)*bits:(i*c.slots+j+1)*bits] for j in range(c.slots)] for i in range(len(p.instances))])
            for t,status in zip(jobs[0]['transitions'],state['transition_status']):
                counts[status]+=1
                if not status:continue
                b=p.branches[t['branch']];tid=p.instances[t['target']]['template']
                arity=self.library.templates[tid]['arity']
                h=self.cells.intern(tid,values[t['target']][:arity],b['rhs'])
                accepted[branch_node[t['branch']]].add(h)
                if status==2:pending+=1;self.stats['alias_targets_resumed']+=1
            work=dict(source_rows=len(source_handles),temporary_rows=len(p.instances),bits=bits,slots=c.slots,
                      matrices=len(jobs[0]['plan']['blocks']),branches=len(stage),half_inputs=half,
                      logical_matrix_macs=sum(len(b['wires'])**2*b['width'] for b in jobs[0]['plan']['blocks']),
                      feature_rows_built=sum(active),feature_rows_zeroed=len(active)-sum(active))
        else:work=dict(source_rows=len(source_handles),temporary_rows=0,bits=bits,slots=0,matrices=0,branches=0)
        for n,left,right in ready:
            choices=accepted[n]
            if len(choices)>1:raise AssertionError('accepted branches disagree on the entire target closure')
            h=next(iter(choices)) if choices and n not in fallback else self.fallback(self.nodes[n]['op'],left,right)
            if not choices and n not in fallback:self.stats['no_match_contexts_retained']+=1
            self.remember(n,left,right,h)
        self.batches.append(dict(**work,ready_nodes=[n for n,_,_ in ready],statuses=dict(counts),
                                 aliases_resumed=pending,unsupported=unrepresented))

    def remember(self,n,left,right,h,learn=True):
        signature=(self.nodes[n]['op'],left,right)
        if learn and self.specializations is not None:
            self.specializations.observe(*signature,h)
        self.edge_cache[signature]=h
        for endpoint in (left,right):self.cache_uses[endpoint].add(signature)
        self.signatures[n]=signature
        self.publish(n,h)

    def summary(self):
        return deepcopy(dict(backend=self.backend,constructor_facts=len(self.nodes),resolved_facts=len(self.outputs),
                    templates=len(self.library.templates),live_cells=len(self.cells.keys),
                    retired_handles=sum('redirect' in r for r in self.cells.records),
                    library=dict(self.library.stats),cells=dict(self.cells.stats),runtime=dict(self.stats),
                    batches=self.batches,events=self.events,
                    drivers={name:r['source'] for name,r in self.rules.items()},bake_only=self.bake_only,
                    fact_specializations=None if self.specializations is None else self.specializations.report()))

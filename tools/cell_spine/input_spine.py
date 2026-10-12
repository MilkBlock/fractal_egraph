"""Construct initial Cells directly, under the already selected spine/marker ABI.

Native ASTs only. Match certified seeds against the finite input term DAG; a
separate exact cover chooses single-root initializers and retained interfaces.
No original inference rules run here. Native whole-state validation remains the
publication gate. Explicit initial unions retain the existing input path.
"""
from collections import defaultdict
from . import names,markers,takeover as t
from .terms import text,walk,actions,expressions


class InputDAG:
    def __init__(self,data):
        self.nodes=[];self.ids={};self.globals={};self.signatures=data['signatures']
        for wrapped in data['actions']:
            a=wrapped['action'];i=self.add(a['expr'])
            if a['kind']=='let':self.globals[a['name']]=i
    def add(self,e):
        if e['kind']=='var':return self.globals[e['name']]
        if e['kind']=='literal':key=('literal',e['sort'],e['text']);children=[]
        else:
            children=[self.add(a) for a in e['args']];key=('call',e['head'],tuple(children))
        if key not in self.ids:
            self.ids[key]=len(self.nodes);self.nodes.append((e,children))
        return self.ids[key]
    def sort(self,i):
        e,_=self.nodes[i]
        return e['sort'] if e['kind']=='literal' else self.signatures[e['head']]['output']
    def raw(self,i):
        e,children=self.nodes[i]
        return e if e['kind']=='literal' else t.call(names.raw(e['head']),[self.raw(c) for c in children])
    def subtree(self,i):
        e,children=self.nodes[i]
        return ({i} if e['kind']=='call' else set())|set().union(*(self.subtree(c) for c in children))
    def match(self,pattern,i,c,binding,covered):
        if text(pattern) in c['params']:
            p=text(pattern)
            if self.sort(i)!=c['param_sorts'][p] or (p in binding and binding[p]!=i):return False
            binding[p]=i;return True
        e,children=self.nodes[i]
        if pattern['kind']!=e['kind']:return False
        if e['kind']=='literal':return (pattern['sort'],pattern['text'])==(e['sort'],e['text'])
        if pattern['head']!=e['head'] or len(pattern['args'])!=len(children):return False
        covered.add(i)
        return all(self.match(p,j,c,binding,covered) for p,j in zip(pattern['args'],children))


def initializers(data,candidates,chosen,layouts,prepared):
    from .compiler import mapping_to,resolve,summary_fields
    dag=InputDAG(data);result=[];obligations=[];seen=set();skipped=defaultdict(int)
    by_op=defaultdict(list)
    for i,(e,_) in enumerate(dag.nodes):
        if e['kind']=='call':by_op[e['head']].append(i)
    def logical(e):
        if e['kind']!='call':return e
        head=e['head']
        if head.endswith('!!'):head=head[:-2]
        elif head.endswith('!'):head=head[:-1]
        return t.call(head,[logical(a) for a in e['args']])
    def interface(family,values):return family,tuple(text(logical(e)) for e in values)
    for c in candidates:
        target=chosen[c['family']];layout=layouts[c['family']]
        constructible=len(c['roots'])==1 and not any(a['kind']=='union' for a in c['acts'])
        if not constructible:skipped['seed needs a multi-root or equality match']+=1
        if layout.get('representation','canonical')!='canonical':
            skipped['selected storage borrows input rows']+=1;constructible=False
        params={p:t.var(v) for p,v in target['params'].items()}
        forest=[t.fork(e,params) for e in target['roots']]
        aliases=[]
        if constructible:
            root_class=c['value_map'][resolve(c['snapshot'],c['roots'][0])]
            aliases=[e for r in target['roots'] for e in walk(r)
                     if target['value_map'][resolve(target['snapshot'],e)]==root_class]
            if not aliases:skipped['source root has no selected spine representative']+=1;constructible=False
        parameter_map=mapping_to(c,target)
        def existing(e,binding):
            if text(e) in binding:return binding[text(e)]
            if e['kind']=='literal':return dag.ids.get(('literal',e['sort'],e['text']))
            children=[existing(x,binding) for x in e['args']]
            return dag.ids.get(('call',e['head'],tuple(children)))
        def matches(depth,binding,covered,roots):
            if depth==len(c['roots']):
                if all(a['kind']!='union' or (existing(a['args'][0],binding) is not None and
                    existing(a['args'][0],binding)==existing(a['args'][1],binding)) for a in c['acts']):
                    yield binding,covered,roots
                return
            pattern=c['roots'][depth]
            if text(pattern) in c['params']:
                ids=[binding[text(pattern)]] if text(pattern) in binding else [i for i in range(len(dag.nodes)) if dag.sort(i)==c['param_sorts'][text(pattern)]]
            else:ids=by_op[pattern['head']]
            for i in ids:
                new=dict(binding);rows=set(covered)
                if dag.match(pattern,i,c,new,rows):yield from matches(depth+1,new,rows,roots+[i])
        for binding,covered,source_roots in matches(0,{},set(),[]):
            i=source_roots[0]
            slots={};consistent=True
            for raw,j in binding.items():
                name=parameter_map[raw]
                if name in slots and slots[name]!=j:consistent=False
                slots[name]=j
            if not consistent or set(slots)!=set(target['params'].values()):continue
            concrete={p:dag.raw(j) for p,j in slots.items()}
            def cano(e):return t.replace(markers.canonical(e,data['signatures']),concrete)
            roots=[cano(e) for e in forest]
            fields=dict(zip(summary_fields(target),roots+[concrete[p] for p in target['params'].values()]))
            full=roots+[concrete[p] for p in target['params'].values()]
            obligation=interface(c['family'],full)
            obligations.append((obligation,covered))
            if not constructible:continue
            provided={obligation}
            for lens in prepared['lenses']:
                if lens['owner']==c['family']:
                    provided.add(interface(lens['child'],[t.replace(e,fields) for e in lens['args']]))
            emitted=[]
            # Materialize the selected forest and its required equality guards.
            for a in target['acts']:
                if a['kind']=='expr':emitted.append(dict(kind='expr',expr=cano(t.fork(a['expr'],params))))
                else:emitted.append(dict(a,args=[cano(t.fork(e,params)) for e in a['args']]))
            # Inherited consumers may require more than the seed forest.
            for a in prepared['supports'][c['family']][1]:
                if a['kind']=='expr':emitted.append(dict(a,expr=t.replace(markers.canonical(a['expr'],data['signatures']),fields)))
                else:emitted.append(dict(a,args=[t.replace(markers.canonical(e,data['signatures']),fields) for e in a['args']]))
            # Preserve every raw-row invariant used to justify later borrowing.
            required=set(prepared.get('raw_contracts',{}).get(str(c['family']),[]))
            for root in forest:
                for e in walk(root):
                    if text(e) in required:
                        raw=t.replace(e,concrete);canonical=cano(e)
                        emitted.extend([dict(kind='expr',expr=raw),dict(kind='union',args=[raw,canonical])])
            marker=t.call(names.summary(c['family']),[t.replace(markers.canonical(e,data['signatures']),fields) for e in layout['terms']])
            emitted.append(dict(kind='expr',expr=marker))
            alias=cano(t.fork(aliases[0],params))
            physical={text(e) for a in emitted for root in expressions([a]) for e in walk(root) if e['kind']=='call'}
            key=(i,tuple(sorted(covered)),tuple(actions(emitted)),text(alias))
            if key in seen:continue
            seen.add(key)
            result.append(dict(id=len(result),path=c['id'],family=c['family'],root=i,covered=covered,
                required_raw=set().union(*(dag.subtree(j) for j in slots.values())),
                physical=physical,actions=emitted,alias=alias,provided=provided))
    return dag,result,dict(skipped),list(dict.fromkeys((key,tuple(sorted(rows))) for key,rows in obligations))


def project(data,candidates,chosen,layouts,prepared,output):
    """Minimum initializer recipe cover for fixed spines, not a time/RSS model."""
    import gurobipy as gp
    if any(a['action']['kind']=='union' for a in data['actions']):
        return None,dict(status='raw_ingress',reason='initial unions retain original identity and ordering')
    dag,options,skipped,obligations=initializers(data,candidates,chosen,layouts,prepared)
    if not options:return None,dict(status='raw_ingress',reason='no directly constructible certified input Cell',skipped=skipped)
    raw_nodes=[i for i,(e,_) in enumerate(dag.nodes) if e['kind']=='call']
    covers=defaultdict(list);providers=defaultdict(list)
    for o in options:
        providers[o['root']].append(o['id'])
        for i in o['covered']:covers[i].append(o['id'])
    with gp.Model('cell_input_spine') as model:
        model.Params.OutputFlag=0;model.Params.MIPGap=0;model.Params.MIPGapAbs=0;model.Params.Threads=1
        x={o['id']:model.addVar(vtype=gp.GRB.BINARY,name=f'cell_{o["id"]}') for o in options}
        raw={i:model.addVar(vtype=gp.GRB.BINARY,name=f'raw_{i}') for i in raw_nodes}
        for i in raw_nodes:
            model.addConstr(raw[i]+gp.quicksum(x[j] for j in covers[i])>=1)
            for child in dag.nodes[i][1]:
                if child in raw:model.addConstr(raw[i]<=raw[child]+gp.quicksum(x[j] for j in providers[child]))
        # Every recognized input Cell is represented, possibly by a larger
        # initializer covering its seed. No greedy largest-first partition.
        for i in sorted({o['root'] for o in options}):model.addConstr(gp.quicksum(x[j] for j in covers[i])>=1)
        # Fact coverage alone is insufficient for specialized bindings/guards.
        # A known interface must be supplied by a certified ancestor lens, or
        # all its initial facts stay visible to the ordinary ingress rules.
        for n,(key,seed_rows) in enumerate(obligations):
            retained=model.addVar(vtype=gp.GRB.BINARY,name=f'visible_match_{n}')
            for i in seed_rows:model.addConstr(retained<=raw[i])
            model.addConstr(retained+gp.quicksum(x[o['id']] for o in options if key in o['provided'])>=1)
        for i in dag.globals.values():
            if i in raw:model.addConstr(raw[i]+gp.quicksum(x[j] for j in providers[i])>=1)
        for o in options:
            for i in o['required_raw']:model.addConstr(raw[i]>=x[o['id']])
        uses=defaultdict(list)
        for i,v in raw.items():uses[text(dag.raw(i))].append(v)
        for o in options:
            for key in o['physical']:uses[key].append(x[o['id']])
        rows=[]
        for i,vs in enumerate(uses.values()):
            y=model.addVar(vtype=gp.GRB.BINARY,name=f'row_{i}');rows.append(y)
            for v in vs:model.addConstr(y>=v)
            model.addConstr(y<=gp.quicksum(vs))
        model.setObjective(gp.quicksum(rows))
        model.write(str(output/'input-selection.lp'));model.optimize()
        if model.Status!=gp.GRB.OPTIMAL:raise RuntimeError(f'input spine selection failed: {model.Status}')
        selected=[o for o in options if x[o['id']].X>.5];retained={i for i,v in raw.items() if v.X>.5}
        model.write(str(output/'input-selection.sol'))
        report=dict(status='projected',scope='exact initial row-cover objective for fixed canonical spines and marker layouts; current input, not arbitrary future grafts',
            objective_scope='distinct constructor/marker recipes; excludes constant global bindings, indices, RSS and union-dependent coalescing',
            semantic_input_obligations=len(obligations),objective=model.ObjVal,variables=model.NumVars,constraints=model.NumConstrs,options=len(options),
            source_constructor_rows=len(raw_nodes),retained_raw_rows=len(retained),
            skipped=skipped,selected=[{k:sorted(o[k]) if isinstance(o[k],set) else o[k]
                for k in ('id','path','family','root','covered','required_raw')} for o in selected])
    aliases=defaultdict(list)
    for o in selected:aliases[o['root']].append(o['alias'])
    def value(i):
        e,children=dag.nodes[i]
        if e['kind']=='literal':return e
        if i not in retained:
            if not aliases[i]:raise ValueError('a retained input reference has no spine value')
            return aliases[i][0]
        return t.call(names.raw(e['head']),[value(c) for c in children])
    emitted=[dict(kind='let',name=n,expr=value(i)) for n,i in dag.globals.items()]
    emitted += [dict(kind='expr',expr=value(i)) for i in sorted(retained)]
    for o in selected:emitted.extend(o['actions'])
    for i,values in aliases.items():
        for v in values:
            if value(i)!=v:emitted.append(dict(kind='union',args=[value(i),v]))
    # Declarative constructor initializers can share their nested writes. A
    # self-union still creates its argument; turn it into a fact before CSE.
    flat=[]
    for a in emitted:
        if a['kind']=='union' and a['args'][0]==a['args'][1]:
            if a['args'][0]['kind']!='call':continue
            a=dict(kind='expr',expr=a['args'][0])
        flat.append(a)
    nested=set()
    for a in flat:
        for root in expressions([a]):
            for e in walk(root):
                if e['kind']=='call' and (a['kind']!='expr' or e!=root):nested.add(text(e))
    flat=[a for a in flat if a['kind']!='expr' or text(a['expr']) not in nested]
    lines=list(dict.fromkeys(actions(flat)))
    report['emitted_actions']=len(lines)
    return lines,report

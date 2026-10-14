"""Certified constructor-boundary cuts of existing Cell seed forests.

No new closed state or arbitrary Cell pair is conjectured. A known seed entails
every source seed under a typed substitution; the enclosing constructor remains
a residual condition. These are additional runtime interfaces, not trace events.
"""
from collections import defaultdict
import itertools
import json

from .terms import text,walk,expressions


def positive_environment(data):
    """Monotone constructor/equality fragment, checked on the native AST."""
    constructors={op for op,s in data['signatures'].items() if s['kind']=='constructor'}
    def term(e):
        return e['kind'] in ('var','literal') or (e['kind']=='call' and e['head'] in constructors and all(term(a) for a in e['args']))
    if len(data['rules'])!=sum(c['kind']=='rule' for c in data['commands']):return False
    for rule in data['rules']:
        for fact in rule['body']:
            if not all(term(e) for e in (fact['args'] if fact['kind']=='eq' else [fact['expr']])):return False
        for action in rule['head']:
            if action['kind'] not in ('expr','let','union'):return False
            if not all(term(e) for e in ([action['expr']] if action['kind'] in ('expr','let') else action['args'])):return False
    return True


def match(pattern,value,parent,target,binding):
    raw=text(pattern)
    if raw in parent['params']:
        sort=value['sort'] if value['kind']=='literal' else target['signatures'][value['head']]['output']
        if sort!=parent['param_sorts'][raw] or (raw in binding and binding[raw]!=value):return False
        binding[raw]=value;return True
    if pattern['kind']!=value['kind']:return False
    if pattern['kind']=='literal':return (pattern['sort'],pattern['text'])==(value['sort'],value['text'])
    if pattern['kind']!='call' or pattern['head']!=value['head'] or len(pattern['args'])!=len(value['args']):return False
    return all(match(a,b,parent,target,binding) for a,b in zip(pattern['args'],value['args']))


def substitute(e,binding):
    if text(e) in binding:return binding[text(e)]
    if e['kind']=='call':return dict(e,args=[substitute(a,binding) for a in e['args']])
    return e


def guards(candidate,binding):
    return {tuple(sorted(text(substitute(e,binding)) for e in a['args']))
            for a in candidate['acts'] if a['kind']=='union'}


def constructor_nodes(candidate):
    result={}
    def visit(e):
        if text(e) in candidate['params']:return
        if e['kind']=='call':
            result.setdefault(text(e),e)
            for a in e['args']:visit(a)
    for e in expressions(candidate['acts']):visit(e)
    return result


class SeedCuts:
    def __init__(self,candidates):
        self.by_op=defaultdict(list);self.anchors={}
        for p in candidates:
            self.anchors.setdefault(p['family'],p)
            # A multi-root source needs a forest-interface matcher. Its existing
            # observed transitions remain available; do not guess one root.
            if len(p['roots'])==1 and text(p['roots'][0]) not in p['params']:
                self.by_op[p['roots'][0]['head']].append(p)
        self.match_attempts=0

    def cuts(self,c):
        from .compiler import mapping_to
        nodes=constructor_nodes(c);available=set(nodes);required_guards=guards(c,{})
        matches={}
        for raw,node in nodes.items():
            found={}
            for p in self.by_op[node['head']]:
                if p['family']==c['family']:continue
                binding={};self.match_attempts+=1
                if not match(p['roots'][0],node,p,c,binding) or set(binding)!=set(p['params']):continue
                if not guards(p,binding)<=required_guards:continue
                provided={text(substitute(e,binding)) for e in constructor_nodes(p).values()}
                if not provided<=available:continue
                # Certified family coordinates retain slot identity, even when
                # equal root values would otherwise hide distinct input ports.
                mapping=mapping_to(p,self.anchors[p['family']])
                key=(p['family'],tuple(sorted((mapping[k],p['param_sorts'][k],text(v)) for k,v in binding.items())),
                     tuple(sorted(provided)),tuple(sorted(guards(p,binding))))
                found.setdefault(key,dict(parent=p['id'],bindings=binding,provided=sorted(provided)))
            matches[raw]=list(found.values())
        result=[];seen=set()
        for raw,node in nodes.items():
            # One natural cut: the known immediate child Cells of this actual
            # seed constructor. Missing child Cells stay ordinary residual facts.
            # Equal child roots are one instance, not two independent parents.
            children=list(dict.fromkeys(text(e) for e in node['args']))
            domains=[matches[x] for x in children if matches.get(x)]
            if len(domains)<2:continue
            for parents in itertools.product(*domains):
                key=json.dumps(parents,sort_keys=True)
                if key in seen:continue
                seen.add(key);result.append(dict(boundary=raw,parents=list(parents)))
        return result


def wiring(c,descriptor,params,by_id):
    p=by_id[descriptor['parent']];binding=descriptor['bindings']
    subst={raw:text(value,True,c['signatures'],params) for raw,value in binding.items()}
    provided={text(e,True,p['signatures'],subst) for root in expressions(p['acts']) for e in walk(root)}
    return dict(parent=p,subst=subst,provided=provided,native_subst=binding)


def extend(candidates,representations,edges):
    from .compiler import mapping_to,lower_option
    by_id={c['id']:c for c in candidates};groups=defaultdict(list)
    for c in representations:groups[c['family']].append(c)
    # Exact representation normalization already preserves the complete typed
    # seed/guard contract. Repeated historical instances need no repeated match.
    index=SeedCuts(representations)
    for c in candidates:
        for cut_id,cut in enumerate(index.cuts(c)):
            options=[]
            for target in groups[c['family']]:
                params=mapping_to(c,target)
                ws=[wiring(c,p,params,by_id) for p in cut['parents']]
                fs=sorted({w['parent']['family'] for w in ws})
                for ps in itertools.product(*(groups[f] for f in fs)):
                    opt=lower_option(c,target,ws,{p['family']:p for p in ps})
                    opt['id']=f'cut_{c["id"]}_{cut_id}_{len(options)}'
                    opt['proof_kind']='certified_seed_cut';options.append(opt)
            edges.append(dict(id=f'cut_{c["id"]}_{cut_id}',kind='certified_seed_cut',target_unit=c['id'],
                observed_parents=[],seed_cut=cut,options=options))
    boundary_count=extend_boundaries(candidates,representations,edges)
    return dict(seed_match_attempts=index.match_attempts,
        boundary_interfaces=boundary_count,
        scope='typed immediate-child cuts of certified seed constructors; complete guards and residual facts retained; multi-root sources keep observed paths; no new tier0 events or closure families')


def extend_boundaries(candidates,representations,edges):
    """Factor a target through a known Cell's port-visible closure row.

    Unlike a syntactic seed cut, a visible row can be another orientation of
    the selected spine. Require the target closure to contain the entire parent
    seed AND its equality guards. The remaining target facts stay explicit.
    Under the same positive rule environment this admits no extra parent effect.
    No operator names, parent subsets, or permutations are guessed/enumerated.
    """
    from .compiler import mapping_to,lower_option,resolve,parse_param,parent_wirings
    groups=defaultdict(list);index=defaultdict(list);by_id={c['id']:c for c in candidates}
    for p in representations:
        groups[p['family']].append(p)
        state=p['snapshot']
        if not p.get('boundary_positive') or len(p['roots'])!=1 or state.get('ports') or state.get('scope',{}).get('semantics')!='positive-constructor-equality/v1':continue
        # Each visible argument must already be an input port or a literal.
        # Ambiguous/aliased ports not bound by this row fail the full-binding
        # check below; absence of a binding is never treated as a wildcard.
        known={resolve(state,parse_param(p,raw)):parse_param(p,raw) for raw in p['params']}
        known.update({i:dict(kind='literal',sort=v['sort'],text=v['literal'])
                      for i,v in enumerate(state['values']) if v.get('literal') is not None})
        root=resolve(state,p['roots'][0]);seen=set()
        for row in state['rows']:
            if row['op'] in p['markers'] or row['result']!=root or any(a not in known for a in row['args']):continue
            pattern=dict(kind='call',head=row['op'],args=[known[a] for a in row['args']])
            raw=text(pattern)
            if raw not in seen:index[row['op']].append((p,pattern));seen.add(raw)
    def interface_key(parent,binding):
        renaming=mapping_to(parent,groups[parent['family']][0])
        return parent['family'],tuple(sorted((renaming[k],parent['param_sorts'][k],text(v)) for k,v in binding.items()))
    existing=defaultdict(set)
    for c in candidates:
        if c.get('origin'):
            for w in parent_wirings(c,c['params'],by_id):existing[c['id']].add(interface_key(w['parent'],w['native_subst']))
    for edge in edges:
        if edge.get('kind')=='certified_seed_cut':
            for p in edge['seed_cut']['parents']:
                existing[edge['target_unit']].add(interface_key(by_id[p['parent']],p['bindings']))
    count=0
    for c in candidates:
        if not c.get('boundary_positive') or c['snapshot'].get('ports') or c['snapshot'].get('scope',{}).get('semantics')!='positive-constructor-equality/v1':continue
        seen=set();cuts=[]
        for raw,node in constructor_nodes(c).items():
            for parent,pattern in index[node['head']]:
                if parent['family']==c['family'] or parent['snapshot']['scope']!=c['snapshot']['scope']:continue
                binding={}
                if not match(pattern,node,parent,c,binding) or set(binding)!=set(parent['params']):continue
                # An existing multi-parent interface already consumes this
                # exact parent binding. Keep that route instead of generating
                # all weaker parent subsets with new raw-lookup consumers.
                if interface_key(parent,binding) in existing[c['id']]:continue
                try:
                    for act in parent['acts']:
                        values=[resolve(c['snapshot'],substitute(e,binding)) for e in expressions([act])]
                        if act['kind']=='union' and len(set(values))!=1:raise StopIteration
                except (KeyError,StopIteration):continue
                identity=(parent['family'],tuple(sorted((parent['params'][k],text(v)) for k,v in binding.items())),raw)
                if identity in seen:continue
                seen.add(identity)
                cuts.append(dict(boundary=raw,parents=[dict(parent=parent['id'],bindings=binding)],
                    proof=dict(kind='visible-parent-closure-row',row=text(pattern),
                               environment='native-AST-positive-constructor-equality',
                               target_contains_parent_seed_and_guards=True)))
        for i,cut in enumerate(cuts):
            options=[];descriptor=cut['parents'][0];parent=by_id[descriptor['parent']]
            for target in groups[c['family']]:
                ws=[wiring(c,descriptor,mapping_to(c,target),by_id)]
                for source in groups[parent['family']]:
                    opt=lower_option(c,target,ws,{parent['family']:source})
                    opt['id']=f'boundary_{c["id"]}_{i}_{len(options)}';opt['proof_kind']='certified_boundary_cut'
                    options.append(opt)
            edges.append(dict(id=f'boundary_{c["id"]}_{i}',kind='certified_boundary_cut',target_unit=c['id'],
                              observed_parents=[],seed_cut=cut,options=options))
            count+=1
    return count

"""Certified constructor-boundary cuts of existing Cell seed forests.

No new closed state or arbitrary Cell pair is conjectured. A known seed entails
every source seed under a typed substitution; the enclosing constructor remains
a residual condition. These are additional runtime interfaces, not trace events.
"""
from collections import defaultdict
import itertools
import json

from .terms import text,walk,expressions


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
    return dict(seed_match_attempts=index.match_attempts,
        scope='typed immediate-child cuts of certified seed constructors; complete guards and residual facts retained; multi-root sources keep observed paths; no new tier0 events or closure families')

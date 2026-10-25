"""Finite structural footprints along same-sort constructor operands.

This closure is a bitset reachability computation, NOT rule saturation. It
includes all constructor alternatives and never reads an analysis relation.
Cycles terminate because only a finite set of constructor tags can be added.
"""
from collections import defaultdict,deque


def footprints(graph):
    local=defaultdict(set);parents=defaultdict(set);enum=defaultdict(set)
    for op,args,out in graph.rows:
        if not args:enum[out].add(op)
    for op,args,out in graph.rows:
        local[out].add(('head',op))
        for arg in args:
            local[out].update(('enum-argument',tag) for tag in enum.get(arg,()))
            if arg[0]=='Eq' and arg[1]==out[1]:parents[arg].add(out)
    tags=sorted(set().union(*local.values()),key=repr) if local else []
    index={tag:i for i,tag in enumerate(tags)}
    own={v:sum(1<<index[t] for t in ts) for v,ts in local.items()}
    state=dict(own);queue=deque(own);queued=set(own);pushes=0;edge_visits=0
    while queue:
        child=queue.popleft();queued.remove(child);bits=state[child]
        for parent in parents.get(child,()):
            edge_visits+=1;old=state.get(parent,0);new=old|bits
            if new==old:continue
            state[parent]=new;pushes+=1
            if parent not in queued:queue.append(parent);queued.add(parent)
    def decode(bits):
        result=set()
        while bits:
            lsb=bits&-bits;result.add(tags[lsb.bit_length()-1]);bits-=lsb
        return result
    local_tokens={v:decode(bits) for v,bits in own.items()}
    recursive_tokens={v:decode(bits) for v,bits in state.items()}
    return local_tokens,recursive_tokens,{'tags':len(tags),'nodes_with_tags':len(state),
        'same_sort_edges':sum(map(len,parents.values())),'successful_unions':pushes,'edge_visits':edge_visits,
        'max_tags_at_node':max(map(len,recursive_tokens.values()),default=0),
        'termination':'finite monotone bitsets; no iteration or candidate budget',
        'scope':'same-sort constructor reachability, not a semantic purity analysis or a saturated Cell certificate'}


def augment(base,extra):
    # Use identical token names in local/recursive controls; only reach differs.
    out={v:set(ts) for v,ts in base.items()}
    for v,tags in extra.items():out.setdefault(v,set()).update(('context',*t) for t in tags)
    return out

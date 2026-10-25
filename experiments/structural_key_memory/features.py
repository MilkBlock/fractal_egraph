"""ID-invariant coordinates from Analyze's observed constructor spines.

No analysis relation, function value, target membership, or Cell semantic ID is
an input feature. These are common matching-spine coordinates, not live Cell
recognition or certified closed-world properties.
"""
from collections import defaultdict,Counter
import hashlib
import json
from pathlib import Path
import re


def constructors(source):
    # The flattened native program uses simple named sorts in constructor
    # declarations. Reject a declaration this restricted reader cannot parse.
    text=source.read_text()
    matches=list(re.finditer(r'^\(constructor\s+([^\s()]+)\s+\(([^()]*)\)\s+([^\s()]+)',text,re.M))
    if len(matches)!=len(re.findall(r'^\(constructor\s',text,re.M)):raise ValueError('unsupported constructor declaration')
    # Native lowering reifies relations as constructors returning ___NameSort.
    # Those are precisely target analysis labels, not IR constructor structure.
    # Fresh-table support is likewise not an independent structural feature.
    return {m[1]:(m[2].split(),m[3]) for m in matches
            if not m[3].startswith('___') and not m[1].startswith('___')}


class Graph:
    def __init__(self,rows,allowed):
        self.rows=[];self.heads=defaultdict(set);self.by_result=defaultdict(list)
        for op,args,result in rows:
            if op not in allowed:continue
            sorts,out=allowed[op]
            if len(args)!=len(sorts) or result[1]!=out or any(a[1]!=s for a,s in zip(args,sorts)):
                raise ValueError(f'constructor signature mismatch: {op}')
            item=(op,tuple(args),result);self.rows.append(item)
            self.heads[result].add(op);self.by_result[result].append(item)

    def tokens(self,depth,vocabulary=None,parameters=False):
        out=defaultdict(set)
        tails=defaultdict(set)
        if depth>=2:
            for op,args,result in self.rows:
                for j,b in enumerate(args):
                    tails[result].update((op,j,leaf) for leaf in self.heads.get(b,()))
        def put(value,token):
            if vocabulary is None or token in vocabulary:out[value].add(token)
        for op,args,result in self.rows:
            put(result,('root',op))
            if parameters:
                # Intrinsic enum constructors and literal values at a known
                # spine's parameter port. No analysis relation is consulted.
                if not args:out[result].add(('enum',op))
                if vocabulary is None or ('root',op) in vocabulary:
                    for i,a in enumerate(args):
                        for child,child_args,_ in self.by_result.get(a,()):
                            if not child_args:out[result].add(('enum-parameter',op,i,child))
                        if a[0]=='Base':
                            out[result].update(('literal-parameter',op,i,*t) for t in literal_tokens(a))
            if depth==0:continue
            for i,a in enumerate(args):
                put(a,('port',op,i))
                if a==result:put(result,('return-alias',op,i))
                for j in range(i):
                    if a==args[j]:put(result,('alias',op,j,i))
                for child in self.heads.get(a,()):put(result,('path',op,i,child))
                if depth>=2:
                    for middle,j,leaf in tails.get(a,()):put(result,('path2',op,i,middle,j,leaf))
        return out


def analyze_vocabulary(queue,allowed):
    raw=queue.read_bytes();data=json.loads(raw);vocab=set();support=Counter();sources=[]
    for job in data['jobs']:
        if job.get('state')!='Saturated' or job.get('ripen',{}).get('updated') is not False:continue
        path=queue.parent/'cells'/f"CSUnit-{job['candidate_id']:06d}"/'trigger-state.json'
        if not path.exists():raise ValueError(f'missing completed Cell trigger: {path}')
        content=path.read_bytes();state=json.loads(content)
        values=[('Base',v['sort'],v['literal']) if v.get('literal') is not None else ('Eq',v['sort'],str(i))
                for i,v in enumerate(state['values'])]
        hidden=set(state.get('subsumed_rows',[]))
        rows=[(r['op'],tuple(values[a] for a in r['args']),values[r['result']])
              for i,r in enumerate(state['rows']) if i not in hidden]
        local=set().union(*Graph(rows,allowed).tokens(2).values()) if rows else set()
        vocab.update(local);support.update(local)
        sources.append({'cell':job['candidate_id'],'file':str(path),
            'sha256':hashlib.sha256(content).hexdigest(),'tokens':len(local)})
    return vocab,{'queue':str(queue),'queue_sha256':hashlib.sha256(raw).hexdigest(),
        'completed_observations':len(sources),'distinct_tokens':len(vocab),
        'tokens_observed_in_multiple_cells':sum(v>1 for v in support.values()),'sources':sources,
        'scope':'constructor shape vocabulary from completed Analyze observations; NOT saturation certificates or runtime Cell membership'}


def literal_tokens(value):
    _,sort,item=value;tokens=[('literal-type',sort)]
    if sort=='i64':
        n=int(item);tokens += [('integer-sign',(n>0)-(n<0)),('integer-bits',abs(n).bit_length())]
        if n in (-1,0,1):tokens.append(('integer-small',n))
    elif sort=='bool':tokens.append(('boolean',item))
    return tokens


def key_signature(key,node_tokens,root_only=False):
    parts=[]
    for position,value in enumerate(key):
        kind,sort,item=value
        parts.append((position,'sort',sort))
        if kind=='Eq':
            tokens=node_tokens.get(value,())
            parts.extend((position,*t) for t in tokens if not root_only or t[0]=='root')
        else:
            # Literals have semantic content; opaque eclass IDs do not. Use a
            # small declared numerical shape, never an arbitrary ID fingerprint.
            parts.extend((position,*t) for t in literal_tokens(value))
        for j in range(position):
            if key[j]==value:parts.append(('key-alias',j,position))
    return tuple(sorted(parts,key=repr))


def native_graph(tables,allowed):
    return Graph([(op,row[:-1],row[-1]) for op,rows in tables.items() if op in allowed for row in rows],allowed)

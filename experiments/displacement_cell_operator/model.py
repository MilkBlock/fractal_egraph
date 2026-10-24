"""Manual state representation for six actual eggcc tuple rules.

No Join is performed in step(): offset shifts become frequency multipliers;
Add-All-Gets' downward-closed position sets become integer frontiers. Union
requests are decoded at the output boundary. This is NOT a general egraph.
"""
from __future__ import annotations

from dataclasses import dataclass, field
import copy
import numpy as np


@dataclass
class Graph:
    # Nodes have stable tuple identities. Equal alternative Concat enodes are
    # in the SAME node, not distinct candidate identities subsequently joined.
    length: list[int]
    single: dict[int, int] = field(default_factory=dict)
    opaque: set[int] = field(default_factory=set)
    concat: dict[int, list[tuple[int, int]]] = field(default_factory=dict)
    known_length: set[int] = field(default_factory=set)

    @property
    def n(self):
        return len(self.length)

    @property
    def width(self):
        # Includes terminal Add-All-Gets p=len, not only emitted Get indices.
        return max(self.length) + 1

    def validate(self):
        if any(x <= 0 for x in self.length):
            raise ValueError('this experiment admits strictly positive tuple lengths')
        if set(self.single) | self.opaque | set(self.concat) != set(range(self.n)):
            raise ValueError('every declared node must have a known constructor role')
        if any(self.length[n] != 1 for n in self.single):
            raise ValueError('Single has length one')
        if len(set(self.single.values())) != len(self.single):
            raise ValueError('equal Single constructor keys must already be canonicalized')
        if self.opaque & set(self.concat):
            raise ValueError('this fixture contract separates opaque and Concat roles')
        for parent, alternatives in self.concat.items():
            if not alternatives:
                raise ValueError('Concat role requires at least one enode')
            for left, right in alternatives:
                if self.length[parent] != self.length[left] + self.length[right]:
                    raise ValueError('alternative must preserve tuple length')
                if left >= parent or right >= parent:
                    raise ValueError('constructor dependency graph must be an ordered DAG')
        if set(self.single) & self.opaque:
            raise ValueError('Single/opaque roles must be disjoint in this contract')

    def edges(self):
        # Parent -> child with displacement, deduped facts (not proof counts).
        result = set()
        for parent, alternatives in self.concat.items():
            for left, right in alternatives:
                # Native rule requires tuple-length(left) for BOTH children.
                if left in self.known_length:
                    result.add((parent, left, 0))
                    result.add((parent, right, self.length[left]))
        return sorted(result)

    def operator(self):
        """Evaluation of polynomial T(z), shape [frequency, source, target]."""
        result = np.zeros((self.width, self.n, self.n), dtype=np.complex128)
        freq = np.arange(self.width)
        for parent, child, offset in self.edges():
            result[:, parent, child] += np.exp(-2j * np.pi * freq * offset / self.width)
        return result


class ExactnessError(RuntimeError):
    pass


def integral_inverse(spectrum, axis=0):
    value = np.fft.ifft(spectrum, axis=axis)
    rounded = np.rint(value.real)
    error = max(float(np.max(np.abs(value.imag))), float(np.max(np.abs(value.real-rounded))))
    # A diagnostic rejection, not a proof of numerical exactness. All reported
    # results are also checked against exact integers and the native engine.
    if error > 1e-6 or np.max(np.abs(rounded)) > 2**40:
        raise ExactnessError(f'floating spectral decoding unsafe: residual={error}')
    return rounded.astype(np.int64), error


class Operator:
    def __init__(self, graph: Graph):
        graph.validate()
        self.graph = copy.deepcopy(graph)
        self.t = graph.operator()
        # T is nilpotent by the strictly decreasing node order. K sums ALL
        # legal constructor paths, including multiple equivalent spines.
        self.k = np.broadcast_to(np.eye(graph.n), self.t.shape).astype(complex).copy()
        # Bake in dependency order rather than enumerating all path lengths.
        # Runtime propagation remains the fixed batched matrix operation.
        for src in range(graph.n):
            for child in np.flatnonzero(np.any(self.t[:,src,:]!=0,axis=0)):
                self.k[:,src,:] += self.t[:,src,child:child+1]*self.k[:,child,:]
        self.decode_error = 0.0

    def step(self, a):
        self.check_domain(a)
        # a[root,node,offset] -> spectrum[freq,root,node] -> same coordinates.
        h = np.fft.fft(np.moveaxis(a.astype(float), 2, 0), axis=0)
        result, error = integral_inverse(h @ self.t)
        self.decode_error = max(self.decode_error, error)
        return np.moveaxis(result, 0, 2) > 0

    def closure(self, seeds):
        self.check_domain(seeds)
        h = np.fft.fft(np.moveaxis(seeds.astype(float), 2, 0), axis=0)
        result, error = integral_inverse(h @ self.k)
        self.decode_error = max(self.decode_error, error)
        return np.moveaxis(result, 0, 2) > 0

    def check_domain(self, a):
        lengths=np.asarray(self.graph.length)
        valid=(np.arange(self.graph.width)[None,None,:]+lengths[None,:,None]
               <=lengths[:,None,None])
        if np.any(a & ~valid):
            raise ValueError('offset support exceeds the declared root extent')

    def update(self, graph):
        """Exact algebraic DAG edge update: K' = K + K[:,u] phase K[v,:].

        Scalar denominator is one because the new edge cannot introduce a
        cycle. This restriction is checked by Graph.validate, not inferred
        from a small observed graph. Removals use the same formula with -phase.
        """
        graph.validate()
        if graph.n != self.graph.n or graph.width != self.graph.width:
            raise ValueError('capacity/identity changes require an explicit repack')
        old, new = set(self.graph.edges()), set(graph.edges())
        updates = []
        for sign, edges in ((-1, old-new), (1, new-old)):
            for src, dst, offset in sorted(edges):
                phase = sign*np.exp(-2j*np.pi*np.arange(graph.width)*offset/graph.width)
                self.k += (self.k[:, :, src:src+1] * phase[:, None, None]
                           * self.k[:, dst:dst+1, :])
                updates.append((sign, src, dst, offset))
        self.graph = copy.deepcopy(graph)
        self.t = graph.operator()
        return updates


class MatrixState:
    def __init__(self, graph, initial_gets):
        self.op = Operator(graph)
        n, w = graph.n, graph.width
        self.a = np.zeros((n,n,w), dtype=bool)
        self.frontier = np.full((n,n,w), -1, dtype=np.int64)
        self.gets = np.zeros((n,w), dtype=bool)
        for node, index in initial_gets:
            self.gets[node,index] = True
        self.unions = set()
        self.round = 0

    def step(self):
        g = self.op.graph
        old_a = self.a
        self.a = old_a | self.op.step(old_a)
        active_roots = np.flatnonzero(np.any(self.gets, axis=1))
        self.a[active_roots,active_roots,0] = True
        # Both child edges respect the native left-length guard. Masks retain
        # unfinished A rows, so a late length fact can resume them without Join.
        lens = np.asarray(g.length)[None,:,None]
        known = np.array([n in g.known_length for n in range(g.n)])[None,:,None]
        opaque = np.array([n in g.opaque for n in range(g.n)])[None,:,None]
        advancing = (self.frontier >= 0) & known & (self.frontier < lens)
        updated = self.frontier + advancing
        self.frontier = np.maximum(updated, np.where(old_a & opaque, 0, -1))

        # Decode RHS effects only. No search or enumeration of a Cartesian
        # product: each nonzero identifies an already-computed output binding.
        for root,node,offset in np.argwhere(advancing):
            pos = int(updated[root,node,offset]-1)
            self._union(('g',int(root),int(offset)+pos), ('g',int(node),pos))
        for node, leaf in g.single.items():
            self._union(('g',node,0), ('v',leaf))
            for root,offset in np.argwhere(old_a[:,node,:]):
                self._union(('g',int(root),int(offset)), ('v',leaf))
        self.round += 1

    def _union(self, a, b):
        for value in (a,b):
            if value[0] == 'g':
                if value[2] < 0 or value[2] >= self.op.graph.width:
                    raise ValueError('coordinate range exceeded, refusing cyclic wraparound')
                self.gets[value[1],value[2]] = True
        self.unions.add(tuple(sorted((a,b))))

    def snapshot(self):
        a = {tuple(map(int,x)) for x in np.argwhere(self.a)}
        b = {(int(r),int(n),int(o),p)
             for r,n,o in np.argwhere(self.frontier >= 0)
             for p in range(int(self.frontier[r,n,o])+1)}
        gets = {tuple(map(int,x)) for x in np.argwhere(self.gets)}
        return a,b,gets


class ScalarState:
    """Independent set/worklist reference, no polynomial/FFT or frontier trick."""
    def __init__(self, graph, initial_gets):
        self.graph = copy.deepcopy(graph)
        self.a, self.b = set(), set()
        self.gets = set(initial_gets)
        self.unions = set()

    def _union(self,a,b):
        for v in (a,b):
            if v[0]=='g': self.gets.add(v[1:])
        self.unions.add(tuple(sorted((a,b))))

    def step(self):
        g = self.graph
        old_a, old_b, old_g = self.a.copy(),self.b.copy(),self.gets.copy()
        for node,leaf in g.single.items(): self._union(('g',node,0),('v',leaf))
        for root,_ in old_g: self.a.add((root,root,0))
        for root,node,offset in old_a:
            for left,right in g.concat.get(node,[]):
                if left in g.known_length:
                    self.a.add((root,left,offset))
                    self.a.add((root,right,offset+g.length[left]))
            if node in g.single: self._union(('g',root,offset),('v',g.single[node]))
            if node in g.opaque: self.b.add((root,node,offset,0))
        for root,node,offset,pos in old_b:
            if node in g.known_length and pos < g.length[node]:
                self._union(('g',root,offset+pos),('g',node,pos))
                self.b.add((root,node,offset,pos+1))

    def snapshot(self):
        return self.a.copy(),self.b.copy(),self.gets.copy()


class InconsistentCoordinates(ValueError):
    pass


def potentials(graph):
    """Solve h[child]-h[parent]=offset using ONLY enabled rule edges.

    No source array indices, AST occurrence order or native matches are read.
    On failure report the conflicting equation; never select a favorite path.
    Disconnected components get independent origins.
    """
    graph.validate()
    adjacency=[[] for _ in range(graph.n)]
    for src,dst,offset in graph.edges():
        adjacency[src].append((dst,offset))
        adjacency[dst].append((src,-offset))
    h=[None]*graph.n
    for root in range(graph.n):
        if h[root] is not None: continue
        h[root]=0;pending=[root]
        while pending:
            src=pending.pop()
            for dst,offset in adjacency[src]:
                value=h[src]+offset
                if h[dst] is None: h[dst]=value;pending.append(dst)
                elif h[dst]!=value:
                    raise InconsistentCoordinates(
                        f'edge {src}->{dst}: requires h[{dst}]-h[{src}]={offset}, '
                        f'existing difference={h[dst]-h[src]}')
    return np.asarray(h,dtype=np.int64)


class CoordinateOperator:
    """Quotient out the entire offset axis after the potential check succeeds."""
    def __init__(self,graph):
        self.graph=copy.deepcopy(graph)
        self.h=potentials(graph)
        self.t=np.zeros((graph.n,graph.n),dtype=np.float32)
        for u,v,_ in graph.edges(): self.t[u,v]=1
        # Bounded-count Boolean squaring: no path-multiplicity explosion.
        self.k=np.maximum(np.eye(graph.n,dtype=np.float32),self.t)
        while True:
            new=np.minimum(1,self.k@self.k)
            if np.array_equal(new,self.k): break
            self.k=new

    def update(self,graph):
        graph.validate();new_h=potentials(graph)
        if graph.n!=self.graph.n: raise ValueError('new capacity needs repacking')
        old,new=set(self.graph.edges()),set(graph.edges())
        if old-new: raise ValueError('coordinate observer supports insert-only rule edges')
        for u,v,offset in sorted(new-old):
            self.k=np.maximum(self.k,self.k[:,u:u+1]@self.k[v:v+1,:])
            self.t[u,v]=1
        self.graph=copy.deepcopy(graph);self.h=new_h
        return sorted(new-old)


class CoordinateState:
    def __init__(self,graph,initial_gets):
        self.op=CoordinateOperator(graph)
        self.a=np.zeros((graph.n,graph.n),dtype=np.float32)
        self.frontier=np.full((graph.n,graph.n),-1,dtype=np.int64)
        self.gets=np.zeros((graph.n,graph.width),dtype=bool)
        for n,i in initial_gets: self.gets[n,i]=True
        self.unions=set()
        self.round=0

    _union=MatrixState._union

    def step(self):
        g=self.op.graph
        old_a=self.a
        self.a=np.minimum(1,old_a+old_a@self.op.t)
        active=np.flatnonzero(np.any(self.gets,axis=1))
        self.a[active,active]=1
        known=np.array([n in g.known_length for n in range(g.n)])[None,:]
        opaque=np.array([n in g.opaque for n in range(g.n)])[None,:]
        advancing=(self.frontier>=0)&known&(self.frontier<np.asarray(g.length)[None,:])
        updated=self.frontier+advancing
        self.frontier=np.maximum(updated,np.where((old_a>0)&opaque,0,-1))
        for root,node in np.argwhere(advancing):
            offset=int(self.op.h[node]-self.op.h[root]);pos=int(updated[root,node]-1)
            self._union(('g',int(root),offset+pos),('g',int(node),pos))
        for node,leaf in g.single.items():
            self._union(('g',node,0),('v',leaf))
            for root in np.flatnonzero(old_a[:,node]):
                offset=int(self.op.h[node]-self.op.h[root])
                self._union(('g',int(root),offset),('v',leaf))
        self.round+=1

    def snapshot(self):
        a={(int(r),int(n),int(self.op.h[n]-self.op.h[r])) for r,n in np.argwhere(self.a)}
        b={(int(r),int(n),int(self.op.h[n]-self.op.h[r]),p)
           for r,n in np.argwhere(self.frontier>=0) for p in range(int(self.frontier[r,n])+1)}
        return a,b,{tuple(map(int,x)) for x in np.argwhere(self.gets)}


class OccurrenceOperator:
    """Exact coordinate splitting when one node occurs at different offsets.

    Offline reachability creates only (node, offset) coordinates that can occur
    from a seed at some declared node. It does not allocate node*width blindly.
    No numerical phases or approximate equality are needed afterwards.
    """
    def __init__(self,graph):
        graph.validate();self.graph=copy.deepcopy(graph)
        adjacency={n:[] for n in range(graph.n)}
        for u,v,d in graph.edges(): adjacency[u].append((v,d))
        slots={(n,0) for n in range(graph.n)};pending=list(slots);edges=set()
        while pending:
            u,offset=pending.pop()
            for v,delta in adjacency[u]:
                target=(v,offset+delta)
                if target[1]>=graph.width: raise ValueError('occurrence exceeds declared bound')
                edges.add(((u,offset),target))
                if target not in slots: slots.add(target);pending.append(target)
        self.slots=sorted(slots);self.index={x:i for i,x in enumerate(self.slots)}
        self.nodes=np.array([n for n,o in self.slots]);self.offsets=np.array([o for n,o in self.slots])
        self.t=np.zeros((len(slots),len(slots)),dtype=np.float32)
        for src,dst in edges:self.t[self.index[src],self.index[dst]]=1
        self.k=np.maximum(np.eye(len(slots),dtype=np.float32),self.t)
        while True:
            new=np.minimum(1,self.k@self.k)
            if np.array_equal(new,self.k):break
            self.k=new


class OccurrenceState:
    def __init__(self,graph,initial_gets):
        self.op=OccurrenceOperator(graph)
        self.a=np.zeros((graph.n,len(self.op.slots)),dtype=np.float32)
        self.frontier=np.full(self.a.shape,-1,dtype=np.int64)
        self.gets=np.zeros((graph.n,graph.width),dtype=bool)
        for n,i in initial_gets:self.gets[n,i]=True
        self.unions=set();self.round=0

    _union=MatrixState._union

    def step(self):
        g=self.op.graph;nodes=self.op.nodes;offsets=self.op.offsets
        old=self.a;self.a=np.minimum(1,old+old@self.op.t)
        active=np.flatnonzero(np.any(self.gets,axis=1))
        self.a[active,[self.op.index[(int(n),0)] for n in active]]=1
        known=np.array([n in g.known_length for n in nodes])[None,:]
        opaque=np.array([n in g.opaque for n in nodes])[None,:]
        advancing=(self.frontier>=0)&known&(self.frontier<np.array(g.length)[nodes][None,:])
        updated=self.frontier+advancing
        self.frontier=np.maximum(updated,np.where((old>0)&opaque,0,-1))
        for root,slot in np.argwhere(advancing):
            node,offset=map(int,self.op.slots[slot]);pos=int(updated[root,slot]-1)
            self._union(('g',int(root),offset+pos),('g',node,pos))
        for node,leaf in g.single.items():self._union(('g',node,0),('v',leaf))
        single_mask=np.array([n in g.single for n in nodes])[None,:]
        for root,slot in np.argwhere((old>0)&single_mask):
            node,offset=map(int,self.op.slots[slot])
            self._union(('g',int(root),offset),('v',g.single[node]))
        self.round+=1

    def snapshot(self):
        a={(int(r),*self.op.slots[s]) for r,s in np.argwhere(self.a)}
        b={(int(r),*self.op.slots[s],p) for r,s in np.argwhere(self.frontier>=0)
           for p in range(int(self.frontier[r,s])+1)}
        return a,b,{tuple(map(int,x)) for x in np.argwhere(self.gets)}


def equality_pairs(gets, unions, leaves):
    universe = sorted([('g',r,i) for r,i in gets]+[('v',v) for v in leaves])
    parent = {x:x for x in universe}
    def find(a):
        while parent[a]!=a:
            parent[a]=parent[parent[a]]
            a=parent[a]
        return a
    for a,b in unions:
        ra,rb=find(a),find(b)
        parent[ra]=rb
    return {(a,b) for a in universe for b in universe if find(a)==find(b)}

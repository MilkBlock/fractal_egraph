"""Exact, finite-width coordinates for a certified commutative boundary domain.

Compilation depends on template terms and wiring, never on concrete input values.
This is a standalone projection compiler, not an egglog execution engine.
"""
from collections import Counter
from itertools import combinations, combinations_with_replacement


def bit_code(value, bits):
    if not isinstance(value, int) or not 0 <= value < 1 << bits:
        raise ValueError('input identity is outside the chosen finite bit width')
    return tuple(1 if value >> i & 1 else -1 for i in range(bits))


def lift2(x):
    return tuple(x) + tuple(x[i]*x[j] for i, j in combinations(range(len(x)), 2))


def certify_commutative(ast, head):
    """Only an unconditional, unsubsumed swap rewrite grants this certificate."""
    for r in ast['rewrites']:
        a, b = r['lhs'], r['rhs']
        if r['conditions'] or r['subsume'] or a.get('head') != head or b.get('head') != head:
            continue
        args = a['args']
        if len(args) == 2 and all(x['kind'] == 'var' for x in args) and args[0] != args[1] and b['args'] == args[::-1]:
            return dict(head=head, command_ordinal=r['command_ordinal'], source=r['source']['text'])
    raise ValueError('missing unconditional commutativity certificate for '+head)


class Circuit:
    def __init__(self, bits, slots, certificate):
        if bits < 1 or slots < 1 or certificate['head'] != 'Add':
            raise ValueError('invalid coordinate domain')
        self.bits, self.slots, self.certificate = bits, slots, certificate
        self.nodes, self.intern, self.roots = [], {}, []
        self.tape_width = 0
        for slot in range(slots):
            self.node('port', (), bits, 1, slot=slot)

    def node(self, op, args, width, bound, slot=-1):
        key = (op, tuple(args), slot)
        if key not in self.intern:
            self.intern[key] = len(self.nodes)
            self.nodes.append(dict(op=op, args=list(args), width=width, bound=bound,
                                   slot=slot, offset=self.tape_width))
            self.tape_width += width
        return self.intern[key]

    def compile(self, term):
        if term[0] == 'Input':
            if not 0 <= term[1] < self.slots:
                raise ValueError('unknown input slot')
            return 'I', term[1]
        if term[0] != 'Add' or len(term) != 3:
            raise ValueError('constructor outside certified boundary domain')
        children = sorted((self.compile(c) for c in term[1:]), key=lambda p:(p[0], p[1]))
        (sa, a), (sb, b) = children
        x, y = self.nodes[a], self.nodes[b]
        shape = f'A({sa},{sb})'
        if sa != sb:
            # Child shape tags fix the roles. Conjunction is concatenation.
            out = self.node('concat', (a,b), x['width']+y['width'], max(x['bound'],y['bound']))
        elif sa == 'I':
            ids = []
            for child in (a,b):
                ids.append(self.node('lift2', (child,), self.bits*(self.bits+1)//2, 1))
            out = self.node('mean', ids, self.nodes[ids[0]]['width'], 1)
        else:
            # General unordered pair of feature vectors. Exact, but width can grow.
            w = x['width']
            out = self.node('moments', (a,b), w+w*(w+1)//2,
                            max(2*x['bound'],2*x['bound']**2))
        return shape, out

    def evaluate(self, ports, previous=None, changed=None):
        if len(ports) != self.slots or any(len(x) != self.bits for x in ports):
            raise ValueError('wrong input matrix shape')
        if any(v not in (-1,1) for p in ports for v in p):
            raise ValueError('ports must be signed binary codes (including unused padding)')
        changed = set(range(self.slots)) if previous is None else set(changed or ())
        if not changed <= set(range(self.slots)):
            raise ValueError('unknown updated slot')
        if previous is not None and any(tuple(ports[i]) != previous[i] for i in range(self.slots) if i not in changed):
            raise ValueError('unreported input update')
        values, dirty, counts = [], [], Counter()
        for n in self.nodes:
            is_dirty = n['slot'] in changed if n['op']=='port' else any(dirty[a] for a in n['args'])
            dirty.append(is_dirty)
            if previous is not None and not is_dirty:
                values.append(previous[len(values)])
                counts['reused_nodes'] += 1
                continue
            args = [values[a] for a in n['args']]
            if n['op'] == 'port':
                v = tuple(ports[n['slot']])
                counts['port_components_read'] += len(v)
            elif n['op'] == 'lift2':
                v = lift2(args[0]); counts['products'] += len(v)-self.bits
            elif n['op'] == 'mean':
                v = tuple((a+b)//2 for a,b in zip(*args)); counts['adds'] += len(v)
                counts['half_scales'] += len(v)
            elif n['op'] == 'concat':
                v = args[0]+args[1]; counts['copied_components'] += len(v)
            else:
                a,b = args
                v = tuple(x+y for x,y in zip(a,b)) + tuple(a[i]*a[j]+b[i]*b[j] for i,j in combinations_with_replacement(range(len(a)),2))
                counts['adds'] += len(v)
                counts['products'] += 2*(len(v)-len(a))
            assert len(v) == n['width']
            values.append(v); counts['computed_nodes'] += 1
        return values, dict(counts)

    def serialize(self):
        return dict(bits=self.bits, slots=self.slots, tape_width=self.tape_width,
                    nodes=self.nodes, certificate=self.certificate)


def components(circuit, node):
    n = circuit.nodes[node]
    if n['op'] == 'concat':
        return sum((components(circuit,a) for a in n['args']), [])
    return [node]


def layout(circuit, library, cell_types, mode='all', shared=False):
    """Plan blocks by symbolic wires. No concrete-value dedup or pairwise jobs."""
    ports, channels = [], []
    roots = []
    for t in library['templates']:
        roots.append([circuit.compile(ch['k']) for ch in t['channels']])
    for instance, tid in enumerate(cell_types):
        t = library['templates'][tid]
        port_map = {}
        for local,p in enumerate(t['ports']):
            if mode == 'root' and p['location'] != 'root':
                continue
            port_map[local] = len(ports)
            ports.append(dict(instance=instance,local=local,**p))
        for local,ch in enumerate(t['channels']):
            if ch['port'] in port_map:
                shape, node = roots[tid][local]
                channels.append(dict(instance=instance,local=local,port=port_map[ch['port']],
                                     recipe=ch['recipe'],shape=shape,node=node))
    shapes = sorted({c['shape'] for c in channels})
    queries, requests = [], []
    for shape in shapes:
        wires = sorted({(c['instance'],c['node']) for c in channels if c['shape']==shape})
        member = [i for i,c in enumerate(channels) if c['shape']==shape]
        locations = {w:i for i,w in enumerate(wires)}
        q = dict(shape=shape,wires=wires,channels=member,
                 channel_rows=[locations[(channels[i]['instance'],channels[i]['node'])] for i in member],terms=[])
        parts = [components(circuit,n) if shared else [n] for _,n in wires]
        assert len({len(p) for p in parts}) == 1
        for col in range(len(parts[0])):
            projected = [(i,p[col]) for (i,_),p in zip(wires,parts)]
            domain = sorted(set(projected))
            requests.append(dict(query=len(queries),projected=projected,domain=domain,
                                 width=circuit.nodes[domain[0][1]]['width']))
        queries.append(q)
    # An already requested superset computes every requested subset. No search
    # over arbitrary combinations and no binding-dependent grouping.
    maximal = []
    for r in sorted(requests,key=lambda r:(-len(r['domain']),r['domain'])):
        if not any(r['width']==b['width'] and set(r['domain'])<=set(b['wires']) for b in maximal):
            maximal.append(dict(width=r['width'],wires=r['domain']))
    for r in requests:
        bi = next(i for i,b in enumerate(maximal) if b['width']==r['width'] and set(r['domain'])<=set(b['wires']))
        positions = {w:i for i,w in enumerate(maximal[bi]['wires'])}
        queries[r['query']]['terms'].append(dict(block=bi,rows=[positions[w] for w in r['projected']]))
    for b in maximal:
        b['max_norm'] = max(circuit.nodes[n]['width']*circuit.nodes[n]['bound']**2 for _,n in b['wires'])
        if 4*b['max_norm'] >= 1<<24:
            raise ValueError('FP32 exact-integer distance bound exceeded; choose another layout')
    stats = dict(blocks=len(maximal),matrix_macs=sum(len(b['wires'])**2*b['width'] for b in maximal),
                 matrix_cells=sum(len(b['wires'])**2 for b in maximal),
                 feature_components=sum(len(b['wires'])*b['width'] for b in maximal),
                 norm_products=sum(len(b['wires'])*b['width'] for b in maximal),
                 norm_adds=sum(len(b['wires'])*(b['width']-1) for b in maximal),
                 output_cells=sum(len(q['wires'])**2 for q in queries),
                 retained_channels=len(channels),condition_pair_objects=0,
                 elementwise_distance_terms=sum(len(q['wires'])**2*len(q['terms']) for q in queries))
    guards=[list(library['templates'][tid].get('input_partition',range(library['templates'][tid].get('arity',circuit.slots)))) for tid in cell_types]
    return dict(ports=ports,channels=channels,blocks=maximal,queries=queries,guards=guards,stats=stats)


def check_guards(plan, ports):
    """Check Cell admission conditions, not candidate rule bindings.

    This bounded host-side preflight is outside the matrix matching kernel.
    A runtime should cache it until canonical input identities change.
    """
    failed=[]
    for i,expected in enumerate(plan['guards']):
        values=[tuple(v) for v in ports[i][:len(expected)]]
        partition=[values.index(v) for v in values]
        if partition != expected:
            failed.append(dict(instance=i,expected=expected,observed=partition))
    if failed:
        raise ValueError('Cell input partition invalidates this Bake: '+str(failed))


def execute(plan, tapes, masks=None, validate=True):
    """Integer reference for GPU GEMM + nonnegative-distance fusion."""
    if validate:
        check_guards(plan,tapes)
    matrices, norms, outputs = [], [], []
    for b in plan['blocks']:
        x = [tapes[i][n] for i,n in b['wires']]
        norms.append([sum(v*v for v in row) for row in x])
        matrices.append([[sum(u*v for u,v in zip(a,c)) for c in x] for a in x])
    for qi,q in enumerate(plan['queries']):
        size = len(q['wires']); result = []
        for i in range(size):
            for j in range(size):
                distance = 0
                for term in q['terms']:
                    b = term['block']; a,c = term['rows'][i],term['rows'][j]
                    part = norms[b][a]+norms[b][c]-2*matrices[b][a][c]
                    assert part >= 0
                    distance += part
                result.append(int(distance==0 and (masks is None or masks[qi][i*size+j])))
        outputs.append(result)
    return outputs


def reconstruct(plan, outputs):
    """Oracle/output materialization only; not GPU matching or candidate search."""
    gate = [[0]*len(plan['channels']) for _ in plan['channels']]
    aggregate = [[0]*len(plan['ports']) for _ in plan['ports']]
    for q,out in zip(plan['queries'],outputs):
        size = len(q['wires'])
        for a,i in zip(q['channels'],q['channel_rows']):
            for b,j in zip(q['channels'],q['channel_rows']):
                value = out[i*size+j]
                gate[a][b] = value
                aggregate[plan['channels'][a]['port']][plan['channels'][b]['port']] += value
    return dict(ports=plan['ports'],channels=plan['channels'],gate=gate,aggregate=aggregate)

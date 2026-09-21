"""Bounded Add-only shared ClosedState store; no egglog kernel replacement.

Native symbolic closures supply templates. Global token identity and congruence
are retained. Closure certificates are NOT transferred to instantiated stores.
"""
import argparse
from array import array
import json
from pathlib import Path
import re
import statistics
import sys
import time


def packed(rows):
    return array('Q', (x for row in rows for x in row))


def triples(buf):
    for i in range(0, len(buf), 3):
        yield tuple(buf[i:i + 3])


class UnionFind:
    __slots__ = ("parent",)
    def __init__(self, n):
        self.parent = array('Q', range(n))

    def find(self, x):
        while self.parent[x] != x:
            x = self.parent[x]
        return x

    def union(self, a, b):
        a, b = self.find(a), self.find(b)
        if a == b:
            return False
        self.parent[max(a, b)] = min(a, b)
        return True


class Store:
    __slots__ = ("uf", "index")
    def canonical(self, row):
        return tuple(self.uf.find(x) for x in row)

    def rebuild(self):
        # Congruence only, not rewrite saturation. Full rebuild is intentional.
        while True:
            keys, changed = {}, False
            for a, b, c in self.raw():
                a, b, c = self.canonical((a, b, c))
                if (a, b) in keys:
                    changed |= self.uf.union(c, keys[a, b])
                else:
                    keys[a, b] = c
            if not changed:
                break
        self.compact()
        self.index = {}
        for location, lhs in self.locations():
            self.index.setdefault(lhs, array('Q')).append(location)

    def rows(self):
        return {self.canonical(r) for r in self.raw()}

    def select(self, lhs):
        lhs = self.uf.find(lhs)
        return {self.canonical(r) for loc in self.index.get(lhs, ())
                for r in self.at(loc) if self.uf.find(r[0]) == lhs}

    def join(self):
        # External consumer: Add(x,y,z), Add(z,w,t).
        return {(x, y, z, w, t) for x, y, z in self.rows()
                for _, w, t in self.select(z)}

    def union(self, a, b):
        self.uf.union(a, b)
        self.rebuild()


class Flat(Store):
    __slots__ = ("data",)
    def __init__(self, template, bindings, n, aliases=()):
        self.uf = UnionFind(n)
        self.data = packed(tuple(b[x] for x in r) for b in bindings for r in template)
        for a, b in aliases:
            self.uf.union(a, b)
        self.rebuild()

    def raw(self):
        return triples(self.data)

    def compact(self):
        self.data = packed(sorted(self.rows()))

    def locations(self):
        return ((i, r[0]) for i, r in enumerate(self.raw()))

    def at(self, loc):
        return [tuple(self.data[loc * 3:loc * 3 + 3])]

    def insert(self, row):
        self.data.extend(row)
        self.rebuild()


class Shared(Store):
    __slots__ = ("template", "width", "bindings", "residual")
    def __init__(self, template, bindings, n, aliases=()):
        self.uf = UnionFind(n)
        self.template = packed(template)
        self.width = len(bindings[0])
        self.bindings = packed(bindings)
        self.residual = array('Q')
        for a, b in aliases:
            self.uf.union(a, b)
        self.rebuild()

    @property
    def instances(self):
        return len(self.bindings) // self.width

    def at(self, loc):
        if loc >= self.instances:
            return [tuple(self.residual[(loc-self.instances)*3:(loc-self.instances+1)*3])]
        b = self.bindings[loc*self.width:(loc+1)*self.width]
        return (tuple(b[x] for x in row) for row in triples(self.template))

    def raw(self):
        for loc in range(self.instances + len(self.residual)//3):
            yield from self.at(loc)

    def compact(self):
        # Exact complete-instance redundancy elimination. No row multiplicities.
        seen, bindings = set(), array('Q')
        for loc in range(self.instances):
            rows = {self.canonical(r) for r in self.at(loc)}
            if not rows <= seen:
                bindings.extend(self.uf.find(x) for x in
                                self.bindings[loc*self.width:(loc+1)*self.width])
                seen.update(rows)
        self.bindings = bindings
        self.residual = packed(sorted({self.canonical(r) for r in triples(self.residual)} - seen))

    def locations(self):
        for loc in range(self.instances + len(self.residual)//3):
            for lhs in sorted({self.uf.find(r[0]) for r in self.at(loc)}):
                yield loc, lhs

    def insert(self, row):
        self.residual.extend(row)
        self.rebuild()


def retained_bytes(obj, seen=None):
    """CPython retained object sizes, including buffers/UF/index, excluding code.

    Not RSS or malloc peak; array.__sizeof__ includes allocated buffer capacity.
    """
    seen = set() if seen is None else seen
    if id(obj) in seen:
        return 0
    seen.add(id(obj))
    size = sys.getsizeof(obj)
    if isinstance(obj, dict):
        size += sum(retained_bytes(k, seen) + retained_bytes(v, seen) for k, v in obj.items())
    elif isinstance(obj, (list, tuple, set)):
        size += sum(retained_bytes(x, seen) for x in obj)
    elif hasattr(obj, '__dict__'):
        size += retained_bytes(vars(obj), seen)
    else:
        for cls in type(obj).__mro__:
            for slot in getattr(cls, '__slots__', ()):
                size += retained_bytes(getattr(obj, slot), seen)
    return size


def resolve(term, state):
    # Restricted ground-term reader, not an egglog parser. Reject unsupported
    # syntax instead of guessing token identity. C3 contains only Add/marker/i64.
    tokens = re.findall(r'\(|\)|[^\s()]+', term)
    it = iter(tokens)
    def read(token):
        if token == '(':
            op, args = next(it), []
            while (t := next(it)) != ')':
                args.append(read(t))
            matches = {r['result'] for r in state['rows'] if r['op'] == op and r['args'] == args}
        else:
            if not re.fullmatch(r'-?\d+', token):
                raise ValueError(f'unsupported ground literal: {token}')
            matches = {i for i,v in enumerate(state['values'])
                       if v['sort'] == 'i64' and v['literal'] == token}
        if len(matches) != 1:
            raise ValueError(f'non-unique or absent term: {term}')
        return matches.pop()
    result = read(next(it))
    if next(it, None) is not None:
        raise ValueError('trailing tokens')
    return result


def load(catalog, state_id):
    cat = json.loads(catalog.read_text())
    state = json.loads((catalog.parent / 'states' / f'state-{state_id:04}.json').read_text())
    if state.get('subsumed_rows'):
        raise ValueError('Add storage probe does not implement subsumed-row matching')
    triggers = [t for t in cat['triggers'] if t['closed_state'] == state_id]
    slots = {i: k for k,(i,v) in enumerate((i,v) for i,v in enumerate(state['values']) if v['sort']=='Math')}
    template = [tuple(slots[x] for x in r['args']+[r['result']]) for r in state['rows'] if r['op']=='Add']
    if not template or any(r['op'] not in ('Add', triggers[0]['binding_origin']['parameter_marker']) for r in state['rows']):
        raise ValueError('probe supports only Add and the boundary marker')
    ids, bindings, aliases = {}, [], []
    def intern(key):
        if key not in ids:
            ids[key] = len(ids)
        return ids[key]
    for ti,t in enumerate(triggers):
        source = json.loads((Path(t['source'])/'closed-state.json').read_text())
        origin, b = t['binding_origin'], [None]*len(slots)
        for v in origin['symbolic_values']:
            if v['sort'] != 'Math':
                raise ValueError('unsupported source token sort')
            slot = slots[t['value_map'][resolve(v['term'],source)]]
            handle = intern((origin['history'],v['token']))
            if b[slot] is not None:
                aliases.append((b[slot],handle))
            else:
                b[slot] = handle
        for i in range(len(b)):
            if b[i] is None:
                b[i] = intern(('synthetic',ti,i))
        bindings.append(b)
    return template, bindings, len(ids), aliases


def timed(fn, repeats=9):
    values=[]
    for _ in range(repeats):
        start=time.perf_counter_ns(); fn(); values.append((time.perf_counter_ns()-start)/1000)
    return statistics.median(values)


def benchmark(data):
    flat, shared = Flat(*data), Shared(*data)
    def equal():
        assert flat.rows() == shared.rows()
        for lhs in range(data[2]):
            assert flat.select(lhs) == shared.select(lhs)
        assert flat.join() == shared.join()
    equal()
    result = {'scope':'standalone Python Add tuple storage over native symbolic closures; no kernel substitution',
              'closed_after_instantiation':'unverified; congruence maintenance does not run rewrites',
              'input_instances':len(data[1]),'template_rows':len(data[0]),'slots_per_instance':len(data[1][0]),
              'global_handles':data[2], 'unique_rows':len(flat.rows()),'retained_instances':shared.instances,
              'memory_measure':'sys.getsizeof recursive retained objects including array capacities, UF and index; not RSS',
              'flat_bytes':retained_bytes(flat),'shared_bytes':retained_bytes(shared),
              'flat_payload_bytes':len(flat.data)*8,
              'shared_payload_bytes':(len(shared.template)+len(shared.bindings)+len(shared.residual))*8,
              'timing_unit':'microseconds, median of 9, Python implementation only',
              'flat_build_us':timed(lambda:Flat(*data)), 'shared_build_us':timed(lambda:Shared(*data)),
              'flat_all_lhs_queries_us':timed(lambda:[flat.select(i) for i in range(data[2])]),
              'shared_all_lhs_queries_us':timed(lambda:[shared.select(i) for i in range(data[2])]),
              'flat_join_us':timed(flat.join), 'shared_join_us':timed(shared.join)}
    # Retained bytes are measured before temporary result materialization.
    operations = [('insert', (0, 0, data[2]-1)),('union',(0,1)),('union',(1,2)),('union',(2,data[2]-1))]
    result['mutation_checks']=[]
    for op,args in operations:
        elapsed=[]
        for store in (flat,shared):
            start=time.perf_counter_ns(); getattr(store,op)(*args) if op=='union' else store.insert(args)
            elapsed.append((time.perf_counter_ns()-start)/1000)
        equal()
        result['mutation_checks'].append({'op':op,'args':args,'unique_rows':len(flat.rows()),
            'flat_us':elapsed[0],'shared_us':elapsed[1],'flat_bytes':retained_bytes(flat),'shared_bytes':retained_bytes(shared)})
    result['retained_byte_saving']=1-result['shared_bytes']/result['flat_bytes']
    return result


if __name__ == '__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('catalog',type=Path,nargs='?');p.add_argument('--state',type=int,default=0)
    p.add_argument('--dataset',type=Path);p.add_argument('--save-dataset',type=Path)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args()
    if bool(a.catalog) == bool(a.dataset):
        p.error('supply exactly one catalog or --dataset')
    data = json.loads(a.dataset.read_text())['data'] if a.dataset else load(a.catalog,a.state)
    if a.save_dataset:
        a.save_dataset.parent.mkdir(parents=True,exist_ok=True)
        a.save_dataset.write_text(json.dumps({'source_catalog':str(a.catalog),'state':a.state,
            'schema':'add-template-bindings-handles-aliases/v1','data':data},indent=2)+'\n')
    result=benchmark(data)
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(result,indent=2)+'\n'); print(json.dumps(result,indent=2))

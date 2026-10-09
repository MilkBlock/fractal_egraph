"""Exact joint-key touch joins; retain source witnesses and reject stale packets.

Only the pure Program value domain is implemented. This is not an egraph
canonicalization service or a general rule-pattern matcher.
"""
from dataclasses import dataclass


def _fresh(packet,runtimes):
    runtime=runtimes.get(packet.instance_key)
    if runtime is None or not packet.valid(runtime):
        raise ValueError(f'stale or missing instance: {packet.instance_key!r}')


@dataclass(frozen=True)
class Joined:
    left: object
    right: object
    key: tuple

    def observe(self,program,outputs,mapping,runtimes):
        """Feed exposed values into a new consumer without copying either Cell."""
        _fresh(self.left,runtimes);_fresh(self.right,runtimes)
        values={}
        for name,(side,node) in mapping.items():
            if side not in ('left','right'):raise ValueError('side must be left or right')
            packet=self.left if side=='left' else self.right
            values[name]=packet.value(node)
        return program.instance(values).observe(outputs)


def join_packets(left,right,links,runtimes):
    """Hash one tuple key, never independently join marginal column sets.

    links are explicit typed port equalities from the consumer. Output preserves
    every input row pair, including different instances with equal values.
    """
    left,right,links=list(left),list(right),tuple(links)
    if not links:raise ValueError('a touch join requires at least one explicit connection')
    schemas=[]
    for packets,field_index in [(left,0),(right,1)]:
        schema=None
        owner=None
        for packet in packets:
            _fresh(packet,runtimes)
            if owner is not None and packet.program is not owner:
                raise ValueError('one join side must use one Program port namespace')
            owner=packet.program
            kinds=[]
            for pair in links:
                if len(pair)!=2:raise ValueError('each link must contain two port IDs')
                node=pair[field_index]
                packet.value(node)  # Reject unexposed fields, not just unknown IDs.
                kinds.append(packet.program._node(node).kind)
            kinds=tuple(kinds)
            if schema is not None and schema!=kinds:raise TypeError('inconsistent port kinds in one side')
            schema=kinds
        schemas.append(schema)
    if all(schema is not None for schema in schemas) and schemas[0]!=schemas[1]:
        raise TypeError('linked port kinds differ')
    index={}
    for packet in right:
        key=tuple(packet.value(r) for _,r in links)
        index.setdefault(key,[]).append(packet)
    joined=[]
    for packet in left:
        key=tuple(packet.value(l) for l,_ in links)
        joined.extend(Joined(packet,other,key) for other in index.get(key,()))
    return joined

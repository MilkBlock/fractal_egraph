"""Discover shared operator fragments and observe correlated boundary packets.

This is a deterministic decomposition of the standalone scalar Program DAG,
not e-graph matching, optimal fragment selection, or committed rule mutations.
A consumer is one named collection of output ports. Operators demanded by the
same two or more consumers are grouped into weakly connected components. There
is no length threshold: a component may contain one operator or a long chain.
Inputs and constants are boundary values, never fragment members.

Plans only reference existing node IDs. Observation uses the Program's existing
Runtime cache once for all roots; fragments do not clone or re-evaluate a DAG.
Rediscover after adding consumers to get the new decomposition. An older plan
remains a valid snapshot of its original consumers and fragment boundaries.
"""
from collections.abc import Mapping
from dataclasses import dataclass
from types import MappingProxyType

from program import Program, Runtime, _name


@dataclass(frozen=True)
class Fragment:
    """One component, with original IDs in topological order.

    inputs are arguments crossing into the component; outputs include every
    member used outside it, even by an unrequested consumer, and requested roots.
    input_support lists the transitive Program input nodes affecting this row.
    """

    id: str
    nodes: tuple
    inputs: tuple
    outputs: tuple
    consumers: tuple
    input_support: tuple


@dataclass(frozen=True)
class Packet:
    """An indivisible row of boundary values from one named instance.

    Each inputs/outputs/bindings field is a tuple of (node ID, scalar value)
    pairs. Keeping these tuples in one packet preserves their joint witness;
    independently combining projected value sets would lose that correlation.
    bindings records transitive root inputs for conservative freshness checking.
    The caller supplies a stable, hashable instance_key and routes that key to
    its Runtime when validating or joining packets.
    """

    program: Program
    instance_key: object
    fragment_id: str
    inputs: tuple
    outputs: tuple
    bindings: tuple

    def value(self, node_id):
        """Read an exposed input/output port, rejecting bool-as-int node IDs."""
        self.program._node(node_id)
        for port, value in self.inputs + self.outputs:
            if port == node_id:
                return value
        raise ValueError(f"node {node_id} is not exposed by {self.fragment_id}")

    def valid(self, runtime):
        """Check current support bindings without traversing or evaluating.

        An unrelated input update leaves the packet valid. A changed support
        input invalidates it even if an output happens to remain equal. Restoring
        the same typed bindings validates it again because Program is pure.
        Instance-key ownership is the caller's responsibility; a Runtime itself
        has no external instance key.
        """
        if not isinstance(runtime, Runtime) or runtime.program is not self.program:
            return False
        for node_id, value in self.bindings:
            if node_id not in runtime.cache:
                return False
            current = runtime.cache[node_id]
            if type(current) is not type(value) or current != value:
                return False
        return True


@dataclass(frozen=True)
class Plan:
    program: Program
    consumers: Mapping
    fragments: tuple

    def observe(self, runtime, instance_key):
        """Return all consumer values and a tuple of full boundary packets.

        Consumer/port names are kept separate, so duplicate names across
        consumers cannot overwrite one another. Exactly one Runtime.observe call
        handles the union of roots and retains its normal evaluation trace.
        """
        if not isinstance(runtime, Runtime) or runtime.program is not self.program:
            raise ValueError("runtime must belong to the discovered Program")
        try:
            hash(instance_key)
        except TypeError as exc:
            raise TypeError("instance_key must be hashable") from exc
        roots = {
            node: node for ports in self.consumers.values() for node in ports.values()
        }
        values = runtime.observe(roots)
        consumers = {
            name: {port: values[node] for port, node in ports.items()}
            for name, ports in self.consumers.items()
        }

        def snapshot(nodes):
            return tuple((node, runtime.cache[node]) for node in nodes)

        packets = tuple(
            Packet(self.program, instance_key, fragment.id,
                   snapshot(fragment.inputs), snapshot(fragment.outputs),
                   snapshot(fragment.input_support))
            for fragment in self.fragments
        )
        return {"consumers": consumers, "packets": packets}


def discover(program, consumers):
    """Find all shared demand components without modifying the Program.

    Demand is propagated backward as a bitset per consumer, rather than per
    port: two aliases in one consumer do not establish inter-consumer sharing.
    Component membership is syntactic and structural; no algebraic identities,
    profitability estimates, search budgets, or fragment-size cutoffs are used.
    """
    if not isinstance(program, Program):
        raise TypeError("program must be a Program")
    if not isinstance(consumers, Mapping):
        raise TypeError("consumers must map names to output port mappings")
    for name in consumers:
        _name(name)
    copied = {}
    for name in sorted(consumers):
        ports = consumers[name]
        if not isinstance(ports, Mapping):
            raise TypeError(f"consumer {name!r} must map port names to node IDs")
        for port, node in ports.items():
            _name(port)
            program._node(node)
        copied[name] = MappingProxyType(dict(sorted(ports.items())))
    consumer_names = tuple(copied)
    demand = [0] * len(program.nodes)
    roots = set()
    for index, ports in enumerate(copied.values()):
        for node in ports.values():
            demand[node] |= 1 << index
            roots.add(node)
    for node in range(len(program.nodes) - 1, -1, -1):
        if demand[node]:
            for arg in program.nodes[node].args:
                demand[arg] |= demand[node]

    members = {
        node for node, mask in enumerate(demand)
        if mask and mask & (mask - 1)
        and program.nodes[node].op not in ("input", "const")
    }
    pending_members = set(members)
    components = []
    for seed in sorted(members):
        if seed not in pending_members:
            continue
        pending_members.remove(seed)
        pending, component = [seed], []
        while pending:
            node = pending.pop()
            component.append(node)
            neighbours = set(program.nodes[node].args) | program._users[node]
            for neighbour in neighbours:
                if neighbour in pending_members and demand[neighbour] == demand[seed]:
                    pending_members.remove(neighbour)
                    pending.append(neighbour)
        components.append(tuple(sorted(component)))

    # Compute support metadata once, without reading any Runtime or evaluating
    # operators. The topological node IDs make recursive graph walks unnecessary.
    support = {}
    for node, mask in enumerate(demand):
        if not mask:
            continue
        operator = program.nodes[node]
        if operator.op == "input":
            support[node] = frozenset((node,))
        else:
            support[node] = frozenset().union(*(support[arg] for arg in operator.args))

    fragments = []
    for nodes in components:
        inside = set(nodes)
        inputs = tuple(sorted({
            arg for node in nodes for arg in program.nodes[node].args if arg not in inside
        }))
        outputs = tuple(node for node in nodes
                        if node in roots or any(user not in inside
                                                for user in program._users[node]))
        names = tuple(name for index, name in enumerate(consumer_names)
                      if demand[nodes[0]] & (1 << index))
        input_support = tuple(sorted(frozenset().union(*(support[node] for node in inputs))))
        fragment_id = "touch:" + ",".join(map(str, nodes))
        fragments.append(Fragment(fragment_id, nodes, inputs, outputs, names, input_support))
    return Plan(program, MappingProxyType(copied), tuple(fragments))

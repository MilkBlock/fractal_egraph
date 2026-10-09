"""Typed, shared operator DAG with exact incremental scalar interpretation.

This is a standalone prototype, not an egglog mutation or extraction engine.
Cells only name DAG ports: one runtime cache belongs to the whole program.
Integers are arbitrary signed Python integers; bool is a distinct type. Backend
lowering must enforce its own representable domain. Build through Program's
methods; direct mutation of nodes, cells, inputs, or runtime.cache is unsupported.
"""
from dataclasses import asdict, dataclass
from heapq import heappop, heappush


@dataclass(frozen=True)
class Node:
    op: str
    kind: str
    args: tuple = ()
    value: object = None


def _scalar(value, kind=None):
    actual = "bool" if type(value) is bool else "int" if type(value) is int else None
    if actual is None or kind is not None and actual != kind:
        raise TypeError(f"expected {kind or 'int or bool'}, got {type(value).__name__}")
    return actual


def _name(name):
    if not isinstance(name, str) or not name:
        raise ValueError("names must be nonempty strings")


class Program:
    """Append-only structurally interned DAG; IDs are topologically ordered.

    Argument order is preserved (no algebraic rewriting or commutative sorting).
    Aliased cells and ports do not allocate nodes or runtime state. New consumers
    can be added while instances exist. New inputs must be bound by patch before
    an existing instance observes consumers of those inputs.
    """

    def __init__(self):
        self.nodes = []
        self.inputs = {}
        self.cells = {}
        self._interned = {}
        self._users = []

    def _node(self, node_id, kind=None):
        if type(node_id) is not int or not 0 <= node_id < len(self.nodes):
            raise ValueError(f"invalid node ID: {node_id!r}")
        node = self.nodes[node_id]
        if kind is not None and node.kind != kind:
            raise TypeError(f"node {node_id} has kind {node.kind}, expected {kind}")
        return node

    def _make(self, op, kind, args=(), value=None):
        node = Node(op, kind, tuple(args), value)
        if node in self._interned:
            return self._interned[node]
        node_id = len(self.nodes)
        self.nodes.append(node)
        self._interned[node] = node_id
        self._users.append(set())
        for arg in node.args:
            self._users[arg].add(node_id)
        return node_id

    def input(self, name, kind="int"):
        _name(name)
        if kind not in ("int", "bool"):
            raise ValueError(f"unsupported kind: {kind}")
        if name in self.inputs:
            node_id = self.inputs[name]
            self._node(node_id, kind)
            return node_id
        node_id = self._make("input", kind, value=name)
        self.inputs[name] = node_id
        return node_id

    def const(self, value):
        return self._make("const", _scalar(value), value=value)

    def add(self, *nodes):
        for node in nodes:
            self._node(node, "int")
        return self._make("add", "int", nodes) if nodes else self.const(0)

    def mul_const(self, factor, node):
        _scalar(factor, "int")
        self._node(node, "int")
        return self._make("scale", "int", (node,), factor)

    def minimum(self, *nodes):
        if not nodes:
            raise ValueError("minimum requires at least one node")
        for node in nodes:
            self._node(node, "int")
        return self._make("min", "int", nodes)

    def eq(self, left, right):
        self._node(right, self._node(left).kind)
        return self._make("eq", "bool", (left, right))

    def lt(self, left, right):
        self._node(left, "int")
        self._node(right, "int")
        return self._make("lt", "bool", (left, right))

    def all(self, *nodes):
        for node in nodes:
            self._node(node, "bool")
        return self._make("and", "bool", nodes) if nodes else self.const(True)

    def bit_all(self, node, mask):
        """True iff all mask bits are set, using Python's signed bitwise rules."""
        self._node(node, "int")
        _scalar(mask, "int")
        return self._make("bit_all", "bool", (node,), mask)

    def name_cell(self, name, ports):
        _name(name)
        if name in self.cells:
            raise ValueError(f"cell already named: {name}")
        for port, node in ports.items():
            _name(port)
            self._node(node)
        self.cells[name] = dict(ports)

    def port(self, cell, name):
        return self.cells[cell][name]

    def reachable_topological(self, roots):
        """Return unique reachable IDs, dependencies first, without evaluating."""
        pending = list(roots)
        for node in pending:
            self._node(node)
        seen = set()
        while pending:
            node = pending.pop()
            if node not in seen:
                seen.add(node)
                pending.extend(self.nodes[node].args)
        return sorted(seen)

    def to_dict(self, roots=None):
        """Serialize stable IDs and optional root closure; aliases stay references."""
        ids = list(range(len(self.nodes))) if roots is None else self.reachable_topological(roots)
        reachable = set(ids)
        return {
            "nodes": [dict(id=i, **asdict(self.nodes[i])) for i in ids],
            "inputs": {name: i for name, i in self.inputs.items() if i in reachable},
            "cells": {name: {port: i for port, i in ports.items() if i in reachable}
                      for name, ports in self.cells.items()},
        }

    def instance(self, inputs):
        return Runtime(self, inputs)


class Runtime:
    """One memo table per instance, shared by every cell and consumer.

    observe traverses missing entries, stopping at cached boundaries. patch
    validates its entire batch, installs all input values atomically, then visits
    only cached users of values
    that changed, once in topological order. An unchanged min (or any operator)
    stops propagation along that edge. Unobserved branches remain unevaluated.
    This sequential interpreter is not thread-safe.

    last_trace records IDs and counts: evaluated means operator evaluations,
    including cold constants; recomputed is the previously cached subset;
    changed compares prior cached values and includes updated input bindings.
    Initial inputs are already cached; cold observation is not a value change.
    observe additionally reports visited: unique IDs reached during dependency
    discovery, including the cached boundaries where traversal stopped.
    """

    def __init__(self, program, inputs):
        self.program = program
        if set(inputs) != set(program.inputs):
            raise ValueError("instance inputs must exactly match the program input names")
        self.cache = {}
        for name, value in inputs.items():
            node_id = program.inputs[name]
            _scalar(value, program.nodes[node_id].kind)
            self.cache[node_id] = value
        self._trace([], [], [])

    def _trace(self, evaluated, recomputed, changed, visited=None):
        trace = dict(evaluated=evaluated, recomputed=recomputed, changed=changed)
        if visited is not None:
            trace["visited"] = visited
        trace["counts"] = {key: len(ids) for key, ids in trace.items()}
        self.last_trace = trace
        return trace

    def _evaluate(self, node_id):
        node = self.program.nodes[node_id]
        args = [self.cache[i] for i in node.args]
        if node.op == "const":
            return node.value
        if node.op == "add":
            return sum(args)
        if node.op == "scale":
            return node.value * args[0]
        if node.op == "min":
            return min(args)
        if node.op == "eq":
            return args[0] == args[1]
        if node.op == "lt":
            return args[0] < args[1]
        if node.op == "and":
            return all(args)
        if node.op == "bit_all":
            return args[0] & node.value == node.value
        raise ValueError(f"cannot evaluate unbound input: {node.value}")

    def observe(self, outputs):
        pending = list(outputs.values())
        for node_id in pending:
            self.program._node(node_id)
        needed, visited = set(), set()
        while pending:
            node_id = pending.pop()
            if node_id in visited:
                continue
            visited.add(node_id)
            if node_id in self.cache:
                continue
            node = self.program.nodes[node_id]
            if node.op == "input":
                raise ValueError(f"input {node.value!r} must be bound by patch")
            needed.add(node_id)
            pending.extend(node.args)
        evaluated = sorted(needed)
        for node_id in evaluated:
            self.cache[node_id] = self._evaluate(node_id)
        self._trace(evaluated, [], [], sorted(visited))
        return {name: self.cache[node] for name, node in outputs.items()}

    def patch(self, updates):
        validated = {}
        for name, value in updates.items():
            if name not in self.program.inputs:
                raise ValueError(f"unknown input: {name}")
            node_id = self.program.inputs[name]
            _scalar(value, self.program.nodes[node_id].kind)
            validated[node_id] = value
        changed = sorted(i for i, value in validated.items()
                         if i not in self.cache or self.cache[i] != value)
        self.cache.update(validated)
        pending, queued, recomputed = [], set(), []

        def enqueue_users(node_id):
            for user in self.program._users[node_id]:
                if user in self.cache and user not in queued:
                    heappush(pending, user)
                    queued.add(user)

        for node_id in changed:
            enqueue_users(node_id)
        while pending:
            node_id = heappop(pending)
            value = self._evaluate(node_id)
            recomputed.append(node_id)
            if value != self.cache[node_id]:
                self.cache[node_id] = value
                changed.append(node_id)
                enqueue_users(node_id)
        return self._trace(list(recomputed), recomputed, sorted(changed))

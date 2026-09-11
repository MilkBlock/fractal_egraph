"""Bounded historical lookahead; predictions do not authorize runtime mutations."""
from dataclasses import dataclass

@dataclass
class Plan:
    path: tuple
    net_gain: int
    library_entry: bool
    compression_gain: int = 0
    startup_bytes: int = 0

def plan(base, first, successors, score, visited, max_steps=3, state_budget=128, charge_startup=True):
    assert 1 <= max_steps <= 3 and state_budget > 0
    initial = score(base)
    def gain(end):
        s = score(end)
        return (s["raw_bytes"]-initial["raw_bytes"]) - (s["coded_bytes"]-initial["coded_bytes"])
    def entry(end):
        return any(n > initial["library_calls"].get(k, 0) for k, n in score(end)["library_calls"].items())
    def evaluate(path):
        startup, previous, triggered = 0, initial, False
        for end in path:
            current = score(end)
            triggered = triggered or entry(end)
            if charge_startup and not triggered:
                startup += max(0, current["coded_bytes"]-previous["coded_bytes"])
            previous = current
        savings = gain(path[-1])
        return Plan(path, savings-startup, entry(path[-1]), savings, startup)
    best = evaluate((first,))
    pending, explored = [(first,)], 0
    while pending and explored < state_budget:
        path = pending.pop()
        explored += 1
        candidate = evaluate(path)
        if candidate.library_entry and candidate.net_gain > 0 and (candidate.net_gain, -len(path)) > (best.net_gain, -len(best.path)):
            best = candidate
        if len(path) < max_steps:
            pending.extend(path+(n,) for n in sorted(successors.get(path[-1], ()), reverse=True) if n not in visited and n not in path)
    return best, {"explored_states": explored, "budget_exhausted": bool(pending)}

@dataclass
class Reservation:
    epoch: object
    graph_version: object
    pending: list

    def take(self, available, epoch, graph_version):
        if not self.pending:
            return None
        if epoch != self.epoch or graph_version != self.graph_version or self.pending[0] not in available:
            self.pending.clear()
            return None
        return self.pending.pop(0)

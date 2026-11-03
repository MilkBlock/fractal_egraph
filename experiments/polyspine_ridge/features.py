"""Causal, typed, ID-invariant Polyspine views of native pre-stage snapshots.

These are observed interface features, not RipSat certificates. Static writer
sets name possible producers, never assert which rule committed an individual
row. No post-stage row, target table or opaque ID is encoded in a feature.
"""
from collections import defaultdict
import json

STAGES = {
    'type-analysis': ({'type-helpers', 'type-analysis'}, 'HasType'),
    'always-run': ({'type-helpers', 'always-run'}, 'ExprIsPure'),
    'is-resolved': ({'is-resolved'}, 'ExprIsResolved'),
    'terms': ({'terms', 'terms-helpers', 'terms-helpers-helpers'}, 'ExtractedExpr'),
}


def identity(value):
    if 'elements' in value:
        return (value['sort'],'container',tuple(sorted(map(identity,value['elements']),key=repr)))
    return (value['sort'], 'eq', value['eq']) if 'eq' in value else (
        value['sort'], 'literal', json.dumps(value['value'], sort_keys=True))


def footprint(node):
    if 'run' in node:
        return {node['run']}
    if 'sequence' in node:
        return set().union(*(footprint(c) for c in node['sequence']))
    return footprint(node.get('body', node.get('saturate')))


def stage_spans(schedule, trace, first_per_cycle=True):
    """Reconstruct exact Saturate boundaries from a verified native Run trace."""
    at = 0
    spans = []
    def visit(node):
        nonlocal at
        start = at
        if 'run' in node:
            if at >= len(trace) or trace[at]['set'] != node['run']:
                raise ValueError('native schedule/trace mismatch')
            changed = trace[at]['updated']
            at += 1
            return changed
        if 'sequence' in node:
            changed = False
            for c in node['sequence']:
                changed = visit(c) or changed
            return changed
        if 'repeat' in node:
            changed = False
            for _ in range(node['repeat']):
                current = visit(node['body'])
                changed |= current
                if not current:
                    break
            return changed
        if 'saturate' in node:
            changed = False
            while visit(node['saturate']):
                changed = True
            names = footprint(node)
            for name, (members, _) in STAGES.items():
                if names == members:
                    spans.append({'stage': name, 'start': start, 'end': at})
            return changed
        raise ValueError('unsupported schedule')
    for node in schedule:
        visit(node)
    if at != len(trace):
        raise ValueError('unconsumed native trace')
    # Choose the first visit to each declared stage in each outer optimization
    # cycle, independent of outcomes. All other Runs are still executed.
    boundaries = {'state-edge-passthrough', 'all-optimizations', 'cheap-optimizations'}
    cycle = 0
    cycles = []
    for step in trace:
        cycles.append(cycle)
        if step['set'] in boundaries:
            cycle += 1
    chosen = {}
    for span in sorted(spans, key=lambda s: (s['start'], -s['end'])):
        span = dict(span, cycle=cycles[span['start']])
        key=(span['cycle'],span['stage']) if first_per_cycle else (span['start'],span['end'],span['stage'])
        chosen.setdefault(key, span)
    return sorted(chosen.values(), key=lambda s: s['start']), len(spans)


class View:
    def __init__(self, core):
        self.tables = {t['name']: t for t in core['tables']}
        self.ordinary = {t['name'] for t in core['tables'] if t['kind'] == 'constructor'
                         and not t['internal'] and not t['schema'][-1]['name'].startswith('___')
                         and not t['name'].startswith('___')}
        self.writers = defaultdict(set)
        self.readers = defaultdict(set)
        for rs in core['rulesets']:
            for rule in rs['rules']:
                for a in rule['head']:
                    name = a.get('call', {}).get('table')
                    if name:
                        self.writers[name].add(rs['name'])
                for a in rule['body']:
                    name = a.get('call', {}).get('table')
                    if name:
                        self.readers[rs['name']].add(name)

    def extract(self, snapshot, stage, previous_stage, prestate=False):
        members, target = STAGES[stage]
        # Current-stage generated analysis is erased even if left over from an
        # earlier visit. Ordinary IR remains visible, as in the old predictor.
        masked = {name for name, writers in self.writers.items()
                  if writers & members and name not in self.ordinary} | {target}
        allowed_interfaces = set().union(*(self.readers[s] for s in members)) - self.ordinary
        if not prestate:allowed_interfaces-=masked
        heads = defaultdict(set)
        structural = []
        all_rows = [r for r in snapshot['rows'] if not r['subsumed']]
        for r in all_rows:
            if r['table'] in self.ordinary:
                vals = tuple(map(identity, r['values']))
                heads[vals[-1]].add(r['table'])
                if vals[-1][0] == 'Expr' or any(v[0] == 'Expr' for v in vals[:-1]):
                    structural.append((r['table'], vals[:-1], vals[-1]))
        base = defaultdict(set)
        interfaces = defaultdict(set)
        before = set()
        keys = set()
        for r in all_rows:
            name = r['table']
            if name == target:
                before.add(identity(r['values'][0]))
            if name not in allowed_interfaces:
                continue
            vals = tuple(map(identity, r['values']))
            # Never traverse the opaque private return identity of a relation.
            for i, v in enumerate(vals[:-1]):
                if v[0] != 'Expr':
                    continue
                detail = []
                for j, other in enumerate(vals[:-1]):
                    if j == i:
                        continue
                    if other[0] == 'Expr':
                        detail.append(('alias', j, other == v))
                    elif other[1] == 'eq':
                        detail.append(('arg-head', j, other[0], tuple(sorted(heads[other]))))
                    elif other[0] == 'i64':
                        n = int(json.loads(other[2]))
                        detail.append(('number', j, (n > 0) - (n < 0), abs(n).bit_length()))
                    else:
                        detail.append(('arg-type', j, other[0]))
                if prestate and self.tables[name]['kind']=='function':
                    returned=vals[-1]
                    detail.append(('value-heads',returned[0],tuple(sorted(heads[returned]))))
                interfaces[v].add(('interface', name, i, tuple(sorted(self.writers[name])), tuple(detail)))
        for op, args, root in structural:
            if root[0] == 'Expr':
                keys.add(root)
                base[root].add(('root', op))
                for i, v in enumerate(args):
                    for head in heads[v]:
                        base[root].add(('path', op, i, head))
                    for j in range(i):
                        if v == args[j]:
                            base[root].add(('alias', op, j, i))
            for i, v in enumerate(args):
                if v[0] == 'Expr':
                    keys.add(v)
                    base[v].add(('port', op, i))
        seams = defaultdict(set)
        seam_sources = defaultdict(set)
        examples = []
        for op, args, root in structural:
            if root[0] != 'Expr':
                continue
            for i, v in enumerate(args):
                if v[0] != 'Expr':
                    continue
                for token in interfaces[v]:
                    seam = ('seam', op, i, token)
                    seams[root].add(seam)
                    seam_sources[root].add(v)
                    if len(examples) < 8:
                        examples.append({'root': root, 'port': v, 'feature': seam})
                # Reverse view exposes a non-root position without picking a
                # representative of an eclass or erasing its argument role.
                for token in interfaces[root]:
                    seams[v].add(('reverse-seam', op, i, token))
                    seam_sources[v].add(root)
        stage_tokens = {('stage', stage), ('previous-observed-stage', previous_stage)}
        result = []
        for key in sorted(keys):
            local = stage_tokens | interfaces[key]
            poly = local | seams[key]
            if len(seam_sources[key]) > 1:
                poly.add(('multiple-interface-sources', min(4, len(seam_sources[key]))))
            result.append((key, tuple(sorted(base[key], key=repr)), tuple(sorted(stage_tokens, key=repr)),
                           tuple(sorted(local, key=repr)), tuple(sorted(poly, key=repr)), key in before))
        stats = {'rows': len(result), 'structural_rows': len(structural),
                 'masked_tables': [] if prestate else sorted(masked), 'interface_tables': sorted(allowed_interfaces),
                 'roots_with_interfaces': sum(bool(interfaces[k]) for k in keys),
                 'roots_with_seams': sum(bool(seams[k]) for k in keys),
                 'roots_with_multiple_sources': sum(len(seam_sources[k]) > 1 for k in keys),
                 'seam_features': sum(len(seams[k]) for k in keys), 'examples': examples}
        return result, stats


def labels(post, keys, target):
    aliases = {(a['sort'], a['old']): a['now'] for a in post['anchors']}
    truth = {identity(r['values'][0]) for r in post['rows']
             if r['table'] == target and not r['subsumed']}
    return [int((k[0], 'eq', aliases.get((k[0], k[2]), k[2])) in truth) for k in keys]

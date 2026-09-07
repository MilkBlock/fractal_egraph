#!/usr/bin/env python3
"""Dependency-free, deterministic retrieval ablation. No fitted hash decoder."""
import argparse
import collections as co
import csv
import functools
import hashlib
import itertools
import json
import random
import statistics as st
import time
from pathlib import Path


def jaccard(a, b):
    keys = a.keys() | b.keys()
    den = sum(max(a.get(k, 0), b.get(k, 0)) for k in keys)
    return sum(min(a.get(k, 0), b.get(k, 0)) for k in keys) / den if den else 1.0


@functools.lru_cache(None)
def canon(nodes, nports):
    """Exact one-hop alpha canonicalization; SELF is fixed, external ports renamed.
    Each port keeps its sort. No child sharing is discarded. This is an oracle,
    not a proposed efficient production canonicalizer.
    """
    best = None
    best_perm = None
    for perm in itertools.permutations(range(nports)):
        enc = tuple(sorted((op, tuple((sort, -1 if p == -1 else perm[p]) for sort, p in children)) for op, children in nodes))
        if best is None or enc < best:
            best, best_perm = enc, perm
    return best, best_perm


def prepare(x):
    root = x['root']
    ports = sorted({c for n in x['nodes'] for c in n['children']} - {root})
    ids = {c: i for i, c in enumerate(ports)}
    nodes = tuple(sorted((n['op'], tuple((c.split('-')[0], -1 if c == root else ids[c]) for c in n['children'])) for n in x['nodes']))
    assert len(ports) <= 6, 'explicitly bounded exhaustive oracle'
    canonical, perm = canon(nodes, len(ports))
    # Validate actual lossless recovery of this local membership set from template+bindings.
    inverse = {perm[i]: p for i, p in enumerate(ports)}
    decoded = {(op, tuple(root if p == -1 else inverse[p] for _, p in cs)) for op, cs in canonical}
    original = {(n['op'], tuple(n['children'])) for n in x['nodes']}
    assert decoded == original
    x['truth'] = frozenset(canonical)
    x['port_count'] = len(ports)
    cheap, strong = co.Counter(), co.Counter()
    for n in x['nodes']:
        child_ids = n['children']
        eq = tuple(child_ids.index(c) for c in child_ids)
        cheap[(n['op'], eq, tuple(c == root for c in child_ids))] += 1
        colors = tuple(tuple('#literal' if op.lstrip('-').isdigit() else op for op in x['child_ops'][c]) for c in child_ids)
        strong[(n['op'], eq, colors, tuple(c == root for c in child_ids))] += 1
    x['cheap'], x['strong'] = cheap, strong
    # O(edges) sharing-sensitive structural baseline, independent of any trace.
    port_incidence = co.Counter()
    for port in ports + [root]:
        signature = tuple(sorted((n['op'], i) for n in x['nodes'] for i, c in enumerate(n['children']) if c == port))
        if signature:
            port_incidence[(port.split('-')[0], port == root, signature)] += 1
    x['port_incidence'] = port_incidence
    kinds = ['presence', 'frequency', 'roles', 'bindings', 'temporal', 'past', 'no_noop']
    features = {k: co.Counter() for k in kinds}
    for event in x['history']:
        rule = event['rule']
        bindings = sorted(event['bindings'].items())
        values = [v for _, v in bindings]
        equality = tuple((name, values.index(value)) for name, value in bindings)
        # Incidence roles are structural information. Explicit bindings-only control
        # prevents calling all their predictive gain a rule-identity effect.
        incidence = tuple((name, value == root, tuple(sorted((n['op'], i) for n in x['nodes'] for i, c in enumerate(n['children']) if c == value))) for name, value in bindings)
        role = (equality, incidence)
        features['presence'][rule] = 1
        features['frequency'][rule] += 1
        features['roles'][(rule, role)] = 1
        features['bindings'][role] = 1
        features['temporal'][(rule, role, x['round'] - event['round'])] = 1
        if event['round'] < x['round']:
            features['past'][(rule, role)] = 1
        if not rule.startswith('noop-'):
            features['no_noop'][(rule, role)] = 1
    x['features'] = features
    return x


VARIANTS = {
    'structure_cheap': ('cheap', None),
    'structure_strong': ('strong', None),
    'structure_incidence': ('incidence', None),
    'cheap+rule_roles': ('cheap', 'roles'),
    'incidence+rule_set': ('incidence', 'presence'),
    'incidence+rule_roles': ('incidence', 'roles'),
    'history_only': (None, 'roles'),
    'strong+rule_set': ('strong', 'presence'),
    'strong+rule_counts': ('strong', 'frequency'),
    'strong+bindings_only': ('strong', 'bindings'),
    'strong+rule_roles': ('strong', 'roles'),
    'strong+temporal': ('strong', 'temporal'),
    'strong+past_only': ('strong', 'past'),
    'strong+remove_known_noop': ('strong', 'no_noop'),
    'strong+shuffled_history': ('strong', 'shuffled'),
    'strong+shuffled_matched_weight': ('strong', 'shuffled'),
}


def split(xs, mode):
    if mode == 'renamed':
        return [x for x in xs if x['replica'] == 0], [x for x in xs if x['replica'] == 1], [x for x in xs if x['replica'] >= 2]
    if mode == 'unseen_schedule':
        return [x for x in xs if x['replica'] == 0 and x['schedule'] < 4], [x for x in xs if x['replica'] == 1 and x['schedule'] == 4], [x for x in xs if x['replica'] >= 2 and x['schedule'] == 5]
    if mode == 'unseen_shape':
        return [x for x in xs if x['replica'] == 0 and x['shape'] < 12], [x for x in xs if x['replica'] == 1 and x['shape'] in [12, 14]], [x for x in xs if x['replica'] >= 2 and x['shape'] in [13, 15]]
    raise ValueError(mode)


def shuffled_features(group, seed):
    # Shuffle complete per-snapshot feature vectors within each partition, never labels.
    order = list(range(len(group)))
    random.Random(seed).shuffle(order)
    return [group[i]['features']['roles'] for i in order]


def pairs(queries, dictionary, shuffled_q, shuffled_d):
    result = []
    for qi, q in enumerate(queries):
        row = []
        for di, d in enumerate(dictionary):
            similarities = {name: jaccard(q[name], d[name]) for name in ['cheap', 'strong', 'port_incidence']}
            similarities['incidence'] = (similarities['strong'] + similarities['port_incidence']) / 2
            for name in q['features']:
                similarities[name] = jaccard(q['features'][name], d['features'][name])
            similarities['shuffled'] = jaccard(shuffled_q[qi], shuffled_d[di])
            # A deterministic shared alpha coordinate system gives a *feasible*
            # alignment, not minimum edit distance over all port permutations.
            intersection = len(q['truth'] & d['truth'])
            union = len(q['truth'] | d['truth'])
            similarities['overlap'] = intersection / union
            similarities['exact'] = q['truth'] == d['truth']
            similarities['residual'] = len(q['truth'] ^ d['truth'])
            similarities['ports'] = q['port_count']
            row.append(similarities)
        result.append(row)
    return result


def score(p, variant, weight):
    structure, history = VARIANTS[variant]
    if structure is None:
        return p[history]
    if history is None:
        return p[structure]
    return (1 - weight) * p[structure] + weight * p[history]


def evaluate(rows, variant, weight, seed):
    # Identical tie order for every ablation. Five independent orders are reported.
    tie = list(range(len(rows[0])))
    random.Random(seed).shuffle(tie)
    out = []
    for row in rows:
        order = sorted(tie, key=lambda i: score(row[i], variant, weight), reverse=True)
        first = row[order[0]]
        out.append((float(first['exact']), float(first['overlap'] >= .8), first['overlap'], first['residual'], float(any(row[i]['exact'] for i in order[:4]))))
    return [st.mean(v[i] for v in out) for i in range(5)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--data', default='results/rule_history/data.jsonl')
    ap.add_argument('--output', default='results/rule_history')
    args = ap.parse_args()
    start = time.perf_counter()
    xs = [prepare(json.loads(line)) for line in open(args.data)]
    prepared = time.perf_counter() - start
    out = Path(args.output)
    out.mkdir(parents=True, exist_ok=True)
    history_groups = co.defaultdict(set)
    for x in xs:
        history_groups[tuple(sorted(x['features']['presence']))].add(x['truth'])
    summary = {'snapshots': len(xs), 'unique_templates': len({x['truth'] for x in xs}), 'prepare_seconds': prepared, 'splits': {},
               'data_sha256': hashlib.sha256(Path(args.data).read_bytes()).hexdigest(),
               'verification': {'traced_untraced_full_snapshot_comparisons': sum(x.get('trace_verified', False) for x in xs),
                                'local_template_roundtrips': len(xs),
                                'distinct_rule_sets': len(history_groups),
                                'rule_sets_with_multiple_templates': sum(len(v) > 1 for v in history_groups.values())}}
    records = []
    for mode in ['renamed', 'unseen_schedule', 'unseen_shape']:
        dictionary, val, test = split(xs, mode)
        # No test threshold or weight tuning. Shared fixed dictionary for all variants.
        sd = shuffled_features(dictionary, 100)
        pv = pairs(val, dictionary, shuffled_features(val, 101), sd)
        pt = pairs(test, dictionary, shuffled_features(test, 102), sd)
        coverage = st.mean(any(p['exact'] for p in row) for row in pt)
        summary['splits'][mode] = {'dictionary': len(dictionary), 'validation': len(val), 'test': len(test), 'exact_template_coverage': coverage}
        selected_weights = {}
        for variant in VARIANTS:
            weights = [0] if VARIANTS[variant][1] is None else ([1] if VARIANTS[variant][0] is None else [0, .1, .25, .5, .75, 1])
            # Optimize exact-template retrieval on validation only; smaller weight wins ties.
            weight = max(weights, key=lambda w: (st.mean(evaluate(pv, variant, w, seed)[0] for seed in range(5)), -w))
            if variant == 'strong+shuffled_matched_weight':
                weight = selected_weights['strong+rule_roles']
            selected_weights[variant] = weight
            runs = [evaluate(pt, variant, weight, seed) for seed in range(5)]
            means = [st.mean(r[i] for r in runs) for i in range(5)]
            base = [evaluate(pt, 'structure_strong', 0, seed)[0] for seed in range(5)]
            deltas = [r[0] - b for r, b in zip(runs, base)]
            record = dict(split=mode, variant=variant, weight=weight, exact_top1=means[0], aligned_near_top1=means[1], mean_overlap=means[2], mean_residual_nodes=means[3], exact_top4=means[4], delta_exact_vs_strong=st.mean(deltas), delta_min=min(deltas), delta_max=max(deltas))
            records.append(record)
            print(mode, variant, 'w=', weight, 'exact=', round(means[0], 4), 'delta=', round(st.mean(deltas), 4), flush=True)
    summary['elapsed_seconds'] = time.perf_counter() - start
    summary['rows'] = records
    (out/'summary.json').write_text(json.dumps(summary, indent=2)+'\n')
    with (out/'metrics.csv').open('w') as f:
        w = csv.DictWriter(f, fieldnames=list(records[0]), lineterminator="\n")
        w.writeheader(); w.writerows(records)
    report(summary, out)


def report(summary, out):
    lines = ['# Rule-history retrieval ablation', '',
        'Controlled synthetic arithmetic programs executed by the local egglog kernel. These are current-snapshot retrieval results, not causal effects, production compression measurements, or hash decoding success.', '',
        f"{summary['snapshots']} snapshots; {summary['unique_templates']} exact one-hop alpha templates; preparation {summary['prepare_seconds']:.3f}s. Each local template was decoded with its original port bindings and checked against the source nodes.", '',
        'Exact target: identical local node sets up to a globally consistent, sort-preserving external-port renaming; SELF fixed. This preserves sharing and self-reference but does not establish semantic equivalence or recursive graph isomorphism.', '',
        'Near target: Jaccard >= 0.8 under independently canonicalized port coordinates. This is a feasible alignment only, not optimal approximate graph matching. Residual is a node count, not compressed bytes.', '',
        'Structure-cheap uses per-node operator, within-node equality and SELF flags. Structure-strong additionally sees child operator sets. Structure-incidence averages strong similarity and the similarity of per-port operator/argument-position incidence histograms, computed without any history. History uses real Survived logical matches, not committed changes. Roles combine variable equalities and their current node-position incidences. Bindings-only removes rule identity from those role features. Past-only excludes the current round but re-canonicalizes old IDs at the current snapshot. Known-noop removal removes only the explicit noop rules.', '',
        'Weights selected using validation exact-top1 only; no test tuning. Five paired dictionary tie orders. Delta ranges below are tie-order sensitivity, NOT confidence intervals. Independent leaf-renaming runs are intentionally easy repeats; they do not demonstrate new-program generalization.', '',
        '## Results', '']
    for mode, spec in summary['splits'].items():
        lines.extend([f'### {mode}', '', f"Dictionary / validation / test = {spec['dictionary']} / {spec['validation']} / {spec['test']}; exact-template availability ceiling = {spec['exact_template_coverage']:.1%}.", '', '| Variant | weight | exact @1 | near @1 | exact @4 | residual nodes | Δ exact vs strong | paired range |', '|---|---:|---:|---:|---:|---:|---:|---:|'])
        for r in summary['rows']:
            if r['split'] != mode: continue
            lines.append(f"| {r['variant']} | {r['weight']} | {r['exact_top1']:.1%} | {r['aligned_near_top1']:.1%} | {r['exact_top4']:.1%} | {r['mean_residual_nodes']:.2f} | {100*r['delta_exact_vs_strong']:+.1f} pp | [{100*r['delta_min']:+.1f}, {100*r['delta_max']:+.1f}] pp |")
        lines.append('')
    lines.extend(['## Scope and reproducibility', '',
        f"Verification: {summary.get('verification', {})}. Dataset SHA-256: `{summary.get('data_sha256', '')}`.", '',
        '16 hand-written expression shapes × 6 schedules × 4 independent renamed runs × 4 rounds. Renamed: replica 0 dictionary, 1 validation, 2–3 test. Unseen schedule: dictionary schedules 0–3/replica 0; validation schedule 4/replica 1; test schedule 5/replicas 2–3. Unseen shape: dictionary shapes 0–11/replica 0; validation shapes 12 and 14/replica 1; test shapes 13 and 15/replicas 2–3. No snapshots from the same execution cross partitions.', '',
        'Root provenance is reconstructed from known benchmark rule LHS terms and retained user-variable bindings. This avoids unstable generated variable names, but is not a generic provenance implementation. Historical roots follow subsequent merges. Event occurrence IDs are not treated as causal order; temporal features use explicit execution rounds.', '',
        'The dictionary is a fixed pool of observations (including repeated templates), equal for every variant. Exact-top4 counts observations, not deduplicated templates. The matched-weight shuffled control uses the weight selected for real rule+role history instead of tuning its own weight. Shuffling occurs independently inside each partition with fixed seeds. Empty history is a real feature: two empty vectors have similarity 1.', '',
        'No estimate of full e-graph compression is reported: child binding tables, primitive payloads, shared dictionary, reconstruction indexes and trace collection overhead would all need accounting. Exhaustive canonicalization is an offline label oracle bounded to at most six external ports, not the deployed retrieval algorithm.', '',
        '```sh', 'cargo run --bin rule_history_data -- results/rule_history/data.jsonl', 'python3 experiments/rule_history/ablate.py', 'python3 -m unittest discover -s experiments/rule_history -p "test_*.py"', '```', '', f"Analysis elapsed: {summary['elapsed_seconds']:.3f}s; default debug collector timings are in the raw data and are not a production performance benchmark."])
    (out/'report.md').write_text('\n'.join(lines)+'\n')


if __name__ == '__main__': main()

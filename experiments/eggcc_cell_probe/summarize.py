#!/usr/bin/env python3
"""Summarize native cell probes without treating overlapping histories as memory savings."""
import collections
import hashlib
import json
import pathlib
import sys

analysis, short_dir, long_dir, dest = map(pathlib.Path, sys.argv[1:])
a = json.loads((analysis / 'analysis.json').read_text())
h = json.loads((analysis / 'history.json').read_text())
short = json.loads((short_dir / 'report.json').read_text())
long = json.loads((long_dir / 'report.json').read_text())
jobs = {j['candidate_id']: j for j in a['saturated_rule_composition']['jobs']}
parent = list(range(len(h['tokens'])))
def find(x):
    while parent[x] != x:
        parent[x] = parent[parent[x]]
        x = parent[x]
    return x
for change in h['changes']:
    for x, y in change['unions']:
        parent[find(x)] = find(y)
def binding(row):
    return tuple(sorted((p['sort'], find(p['token'])) for p in jobs[row['id']]['origin']['parameters']))
def summarize(probe):
    complete = [r for r in probe['rows'] if r['result']['status'] == 'fixed_point']
    groups = collections.defaultdict(list)
    for r in complete:
        groups[r['final_group']].append(r)
    summary = {
        'milliseconds': probe['milliseconds'],
        'statuses': dict(collections.Counter(r['result']['status'] for r in probe['rows'])),
        'completed_initial_groups': len({r['initial_group'] for r in complete}),
        'final_groups': len(groups),
        'groups_with_multiple_occurrences': sum(len(rs) > 1 for rs in groups.values()),
        'occurrences_in_shared_groups': sum(len(rs) for rs in groups.values() if len(rs) > 1),
        'groups_merging_different_initial_states': sum(len({r['initial_group'] for r in rs}) > 1 for rs in groups.values()),
        'groups_with_distinct_recorded_boundary_bindings': sum(len({binding(r) for r in rs}) > 1 for rs in groups.values()),
        'net_row_growth_exceeds_empty_control': sum(r['final_rows'] - r['initial_rows'] > sum(probe['background_tables'].values()) for r in complete),
    }
    return summary, groups
long_summary, groups = summarize(long)
short_summary, _ = summarize(short)
get_group, get_rows = max(groups.items(), key=lambda kv: len(kv[1]))
result = {
    'scope': long['scope'],
    'source_sha256': hashlib.sha256((analysis / 'source.egg').read_bytes()).hexdigest(),
    'selection': {k: long[k] for k in ['prepared_jobs', 'excluded_with_input_initializer', 'selected_jobs', 'input_initializer_rules']},
    'input_initializer_rows': sum(long['input_initializer_tables'].values()),
    'empty_context_rows': sum(long['background_tables'].values()),
    'short_observation': short_summary,
    'long_observation': long_summary,
    'largest_shared_group': {
        'group': get_group, 'occurrences': len(get_rows),
        'recorded_binding_signatures_after_recorded_unions': len({binding(r) for r in get_rows}),
        'initial_rows': sorted({r['initial_rows'] for r in get_rows}),
        'final_rows': get_rows[0]['final_rows'],
        'boundary_slots': get_rows[0]['interface_slots'],
        'tables': get_rows[0]['final_tables'],
        'rules': [long['rules'][i] for i in sorted({i for r in get_rows for i in r['family']})],
    },
    'unfinished_with_ablation': [
        {k: r[k] for k in ['id', 'family', 'initial_rows', 'final_rows', 'result', 'ablation_without_swap_if', 'ablation_rows']}
        for r in long['rows'] if r['result']['status'] == 'unfinished'
    ],
    'grafts': long['grafts'],
    'limitations': [
        'Only prepared entries from one real fact.bril export are tested; the 144 cases are a selected subset, not all eggcc histories.',
        'Equality-sort interfaces remain symbolic; primitive payloads are preserved. Snapshot equality is not a proof for all substitutions.',
        'Occurrences and their interiors can overlap; no RSS, physical compression ratio, or distinct non-overlapping cell coverage is claimed.',
        'Input-slot types and aliasing are preserved up to renaming; production interface adapters and planner/storage integration are not implemented by this probe.',
        'Fixed point is relative to repeatedly executing the original optimizer schedule, excluding its separate input-initialization schedule.',
        'Disabling swap-if is a changed-environment attribution ablation, not proof of saturation in the original environment.',
        'Warm/fresh elapsed times are not speedup measurements: planner and native query cache warmth also differ.',
    ],
}
dest.mkdir(parents=True, exist_ok=True)
(dest / 'results.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
(dest / 'getter-state.json').write_bytes((long_dir / f'group-{get_group}.json').read_bytes())
print(json.dumps({'short': short_summary, 'long': long_summary}, indent=2))

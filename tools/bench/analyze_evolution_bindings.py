#!/usr/bin/env python3
"""Classify binding differences in an existing evolution catalog, without rerunning egglog."""
import argparse
import collections
import hashlib
import json
from pathlib import Path

GUARD = ['source_family', 'rule', 'source_support', 'before_aliases', 'read_guard', 'touched_slots', 'input_refinement']
def key(x):
    return json.dumps(x, sort_keys=True, separators=(',', ':'))
def partition(binding):
    ids = {}
    return [ids.setdefault(key(x), len(ids)) for x in binding]
def analyze(catalog, library):
    groups = collections.defaultdict(list)
    families = {f['id']: f for f in catalog['families']}
    for pattern in catalog['patterns']:
        groups[key({k: pattern['transition'][k] for k in GUARD})].append(pattern)
    result = []
    for patterns in groups.values():
        variants = {key(p['transition']['target_binding']): p['transition']['target_binding'] for p in patterns}
        if len(variants) < 2:
            continue
        ts = [p['transition'] for p in patterns]
        family_ids = {t['target_family'] for t in ts}
        assert len(family_ids) == 1, 'Different target templates require a separate classification'
        family = families[ts[0]['target_family']]
        macro = library['macros'][str(family['origin_rule'])]
        assert macro['template'] == family['rows'] and macro['slots'] == family['slots']
        bindings = list(variants.values())
        inputs = [i for i, (name, _) in enumerate(family['slots']) if not name.startswith('adaptive_')]
        inputs_equal = len({key([b[i] for i in inputs]) for b in bindings}) == 1
        slots = []
        for i, (term, sort) in enumerate(family['slots']):
            values = {key(b[i]): b[i] for b in bindings}
            if len(values) == 1:
                continue
            values = list(values.values())
            assignment = next((line for line in macro['actions'].splitlines() if line.startswith('(let ' + term + ' ')), None)
            slots.append(dict(slot=i, term=term, sort=sort, construction=assignment,
                alternatives=values,
                local_renumbering=len({v['new'] for v in values if 'new' in v}) > 1,
                old_port_reuse=any('new' in v for v in values) and any(any(s.startswith('port_') for s in v.get('post_of', [])) for v in values),
                additional_pre_value_equality=any(len(v.get('post_of', [])) > 1 for v in values)))
        result.append(dict(group=len(result), rule=ts[0]['rule'], source_family=ts[0]['source_family'], target_family=family['id'],
            pattern_ids=[p['id'] for p in patterns], binding_variants=len(bindings),
            input_mapping_invariant=inputs_equal,
            target_alias_partition_invariant=len({key(partition(b)) for b in bindings}) == 1,
            source_alias_partition_invariant=len({key(t['after_aliases']) for t in ts}) == 1,
            differing_slots=slots,
            write_records_also_differ=len({key(t['committed_math_rows_postcanonical']) for t in ts}) > 1,
            recorded_union_lists_vary=len({key(t['traced_read_value_unions']) for t in ts}) > 1))
    return dict(schema='cell-binding-differences/v1', groups=result,
        summary=dict(groups=len(result), input_mapping_invariant=sum(g['input_mapping_invariant'] for g in result),
            target_alias_partition_changes=sum(not g['target_alias_partition_invariant'] for g in result),
            source_alias_partition_changes=sum(not g['source_alias_partition_invariant'] for g in result),
            groups_with_old_port_reuse=sum(any(s['old_port_reuse'] for s in g['differing_slots']) for g in result),
            groups_with_additional_pre_value_equalities=sum(any(s['additional_pre_value_equality'] for s in g['differing_slots']) for g in result),
            groups_with_incidental_local_renumbering=sum(any(s['local_renumbering'] for s in g['differing_slots']) for g in result),
            groups_with_write_differences=sum(g['write_records_also_differ'] for g in result),
            by_rule=dict(collections.Counter(g['rule'] for g in result))),
        scope='Existing observed catalog only. post_of records post-step canonical correspondence; it does not identify the responsible union or establish pre-match existence of an unreferenced result. new labels are local symbols, not allocation events. No runtime pruning or pattern merging is performed.')

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--catalog', type=Path, required=True)
    p.add_argument('--library', type=Path, default=Path('experiments/math_mini3/adaptive_runtime/bake_lib_round2.json'))
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    data = analyze(json.loads(a.catalog.read_text()), json.loads(a.library.read_text()))
    data['catalog_sha256'] = hashlib.sha256(a.catalog.read_bytes()).hexdigest()
    data['library_sha256'] = hashlib.sha256(a.library.read_bytes()).hexdigest()
    data['catalog'] = str(a.catalog)
    a.output.write_text(json.dumps(data, indent=2) + '\n')
    print(json.dumps(data['summary'], indent=2))
if __name__ == '__main__':
    main()

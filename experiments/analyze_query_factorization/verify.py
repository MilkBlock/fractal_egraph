"""Differential native analyze validation with fact-bounded, not timed, prefixes."""
import argparse
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def read(p):
    return json.loads(p.read_text())


def semantic(directory):
    report = read(directory / 'analysis.json')
    q = report['saturated_rule_composition']
    states = {}
    for name in ('saturated-rule-composition.json', 'unfinished-state.json'):
        for p in sorted((directory / 'saturated-rule-composition/cells').glob('*/' + name)):
            state = read(p)
            state.pop('local_ids', None)
            states[p.parent.name + '/' + name] = state
    def decision(e):
        return [e.get('status'), bool(e.get('bindings')), bool(e.get('new_bindings')), e.get('new_effects')]
    return {
        'tier0': report['summary'],
        'counts': q['counts'],
        'states': states,
        'weak': sorted([[e['from'], e['to'], e['same_ripened_family'], e['reentry_observation'], decision(e['evidence'])] for e in q['weak_dominate']]),
        'structural': {k: decision(e) for k, e in q['structural_reentry'].items()},
        'bucket_matches': sorted([[e['source'], e['target'], e['contains_known_C'], e['fresh_match']] for e in q['bucket_trigger_matches']]),
        'unresolved': sorted([[e['from'], e['to'], e['diagnostic']['status']] for e in q['weak_unresolved']]),
    }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--before', type=Path, required=True)
    p.add_argument('--after', type=Path, default=ROOT / 'target/release/egg_layout')
    p.add_argument('--output', type=Path, required=True)
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    results = {}
    for label, binary in [('before', args.before), ('after', args.after)]:
        dest = args.output / label
        cmd = [str(binary.resolve()), 'analyze', '--source', str(ROOT / 'experiments/math_mini3/source.egg'),
               '--output', str(dest.resolve()), '--ripen-observe-ms', '60000', '--ripen-observe-facts', '80']
        with (args.output / (label + '.log')).open('w') as log:
            subprocess.run(cmd, cwd=ROOT, env={**os.environ, 'EGG_LAYOUT_GRAPHS': '0'},
                           stdout=log, stderr=log, check=True, timeout=30)
        results[label] = semantic(dest)
    a, b = results['before'], results['after']
    comparisons = {k: a[k] == b[k] for k in a}
    result = {'scope': 'actual native pipeline; 80-fact ripen prefixes, 60000ms safety bound; not the default-limit timing benchmark',
              'comparisons': comparisons, 'state_files': len(a['states']), 'weak_decisions': len(a['weak']),
              'counts': b['counts'], 'all_equal': all(comparisons.values()),
              'witness_scope': 'existence/freshness and exported graph states; one valid witness may differ with query evaluation order'}
    (args.output / 'comparison.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    assert result['all_equal'], 'see comparison.json and before/after native results'


if __name__ == '__main__':
    main()

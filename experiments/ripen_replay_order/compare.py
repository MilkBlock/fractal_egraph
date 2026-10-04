"""Compare preparation gates on the same captured history, without crediting cells."""
import collections
import json
import pathlib
import sys

def summary(path):
    root = json.loads(pathlib.Path(path).read_text())
    result = root['saturated_rule_composition']
    counts = collections.Counter()
    for job in result['jobs']:
        reason = job.get('reason', '')
        category = ('commit_order' if 'interleave match order' in reason else
                    'passed_replay_export_blocked' if 'export its trigger state' in reason else
                    'missing_scalar_or_binding' if 'scalar literal' in reason else
                    'unresolved_binding' if 'unresolved binding' in reason else
                    'output_alias' if 'output alias failed' in reason else
                    'external_injection' if 'injection failed' in reason else
                    'other')
        counts[category] += 1
    return {'jobs': len(result['jobs']), 'reasons': dict(counts),
            'states': result['counts'], 'preparation': result['preparation_cache'],
            'members': [(job['candidate_id'], job['candidate_kind'])
                        for job in result['jobs']]}

before, after = map(summary, sys.argv[1:3])
# Full members are retained only for the equality check, not in the compact report.
assert before.pop('members') == after.pop('members'), 'composition population changed'
print(json.dumps({'scope': 'Offline native analyze replay; no cell certification or Join speedup claimed',
                  'before': before, 'after': after}, indent=2))

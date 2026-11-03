"""Separate unusable Bake environments from incomplete/uncertified Cells.

The native command may complete successfully while individual preparations fail.
A successful process or valid JSON is not a usable closure certificate.
"""
from collections import Counter
import hashlib
import json
from pathlib import Path
import re

SCHEMA = 'eggfuse-bake-health/v1'
# Native parser/typechecker failures are infrastructure errors in a replay of an
# already typed program. Replay check failures and observation limits are not.
ENVIRONMENT_ERROR = re.compile(
    r'\b(?:Undefined sort|Unbound function|Unbound symbol|Unknown primitive|'
    r'Unknown sort|unknown sort|unknown primitive)\b')

def inspect(directory):
    root = Path(directory)
    files = {n: (root/n).read_bytes() for n in
             ('analysis.json', 'cell-recipes.json', 'cell-device-recipes.json')}
    analysis, recipes, device = (json.loads(files[n]) for n in files)
    if analysis.get('status') not in ('complete', 'observed_unfinished'):
        raise ValueError('Bake analysis did not complete')
    jobs = analysis.get('saturated_rule_composition', {}).get('jobs', [])
    states = Counter(j['state'] for j in jobs)
    errors = [dict(candidate_kind=j.get('candidate_kind'), candidate_id=j.get('candidate_id'),
                   reason=j['reason']) for j in jobs
              if ENVIRONMENT_ERROR.search(j.get('reason', ''))]
    cells = recipes.get('cells', [])
    completions = Counter(c.get('completion_status', 'unknown') for c in cells)
    certified = sum((c.get('completion') or {}).get('certified') is True for c in cells)
    def reason(cell):
        value = cell.get('completion_reason') or 'unspecified'
        if isinstance(value, dict) and isinstance(value.get('reason'), str):
            return value['reason']
        return value if isinstance(value, str) else json.dumps(value, sort_keys=True)
    reasons = Counter(reason(c) for c in cells
                      if c.get('completion') and not c['completion'].get('certified'))
    result = dict(schema=SCHEMA, analysis_status=analysis['status'], environment_valid=not errors,
        jobs=len(jobs), job_states=dict(states), prepared_instances=len(device.get('instances', [])),
        exported_programs=len(device.get('programs', [])), completion_status=dict(completions),
        certified_endpoints=certified,
        live_substitution_authorized=any(c.get('live_substitution_authorized') is True for c in cells),
        environment_errors=errors, certification_rejections=dict(reasons),
        source_program_status=device.get('source_program', {}).get('status', 'missing'),
        fingerprints={n: hashlib.sha256(v).hexdigest() for n, v in files.items()},
        scope='prepared programs and observed endpoints are not certified terminal substitution')
    return result

def require_usable(report):
    if not report['environment_valid']:
        first = report['environment_errors'][0]['reason']
        raise ValueError(f"Bake environment failed for {len(report['environment_errors'])} combinations: "
                         f"{first}. Rebuild the analyzer and recompile; this is not a valid Bake cache.")
    return report

def validate(directory):
    root = Path(directory)
    report = require_usable(inspect(root))
    saved = root/'bake-health.json'
    if saved.exists() and json.loads(saved.read_text()) != report:
        raise ValueError('Bake health receipt differs from the actual analysis')
    return report

def publish(directory):
    root = Path(directory)
    report = inspect(root)
    (root/'bake-health.json').write_text(json.dumps(report, indent=2)+'\n')
    return require_usable(report)

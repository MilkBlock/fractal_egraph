#!/usr/bin/env python3
"""Bounded survey of every historical Use in a saved default-policy analysis.
Each attempt runs the native ripen-use program independently; no raw trace piping.
"""
import argparse
from collections import Counter
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time


def run_bounded(cmd, log, timeout, rss_mb):
    start = time.monotonic()
    peak = 0
    stopped = None
    with log.open('w') as output:
        p = subprocess.Popen(cmd, stdout=output, stderr=subprocess.STDOUT,
                             start_new_session=True)
        while p.poll() is None:
            if time.monotonic() - start > timeout:
                stopped = 'timeout'
            if rss_mb:
                ps = subprocess.run(['ps', '-o', 'rss=', '-p', str(p.pid)],
                                    capture_output=True, text=True)
                if ps.stdout.strip().isdigit():
                    peak = max(peak, int(ps.stdout) * 1024)
                    if peak > rss_mb * 1024 * 1024:
                        stopped = 'memory_budget'
            if stopped:
                try:
                    os.killpg(p.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                break
            time.sleep(.2)
        code = p.wait()
    return code, stopped, time.monotonic() - start, peak


def annotate(result, reuse, uid):
    """Add canonical wire roles from the verified input analysis, for UI only."""
    template = reuse['templates'][reuse['uses'][uid]['template']]
    for m in result['origin']['comb_members']:
        step = template['pattern']['steps'][m['slot']]
        schema = reuse['schemas'][step['schema']]
        if m['rule'] != schema['rule']:
            raise ValueError('analysis rule identity mismatch')
        m.update(parents=sorted(m['parents']), binding=step['wiring'],
                 aliases=step['aliases'], input_roles=schema['input_roles'],
                 output_roles=schema['output_roles'])


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--history', type=Path, required=True)
    p.add_argument('--analysis', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--binary', type=Path, default=Path('target/release/egg_layout'))
    p.add_argument('--rounds', type=int, default=4)
    p.add_argument('--timeout', type=float, default=8)
    p.add_argument('--rss-mb', type=int, default=768)
    p.add_argument('--limit', type=int)
    a = p.parse_args()
    if a.rounds < 1 or a.timeout <= 0 or a.rss_mb < 0 or (a.limit is not None and a.limit < 1):
        p.error('invalid budget')
    r = json.loads(a.analysis.read_text())['layers']['reuse']
    h = json.loads(a.history.read_text())
    if len(h['records']) != r['stats']['events']:
        p.error('history and analysis event counts differ')
    a.output.mkdir(parents=True, exist_ok=False)
    (a.output / 'logs').mkdir()
    uses = r['uses']
    selected = list(range(len(uses)))[:a.limit]
    summary = {'scope': 'Bounded independent native ripen of symbolic interfaces; sharing is conditional on extracted interfaces. Not all possible rule combs or concrete tier0 closures.',
               'history': str(a.history.resolve()), 'analysis': str(a.analysis.resolve()),
               'population': {'historical_uses': len(uses), 'templates': len(r['templates']),
                              'templates_with_uses': len({u['template'] for u in uses}),
                              'active_uses': r['stats']['active_uses'],
                              'templates_without_uses': len(r['templates'])-len({u['template'] for u in uses})},
               'budgets': {'rounds': a.rounds, 'seconds_per_use': a.timeout,
                           'sampled_rss_mb': a.rss_mb, 'rss_poll_seconds': .2},
               'attempts': [], 'counts': {}, 'not_attempted': len(uses), 'status': 'running'}
    closed = []
    binary, history = str(a.binary.resolve()), str(a.history.resolve())
    for uid in selected:
        if shutil.disk_usage(a.output).free < 4 * 1024**3:
            summary['stopped_reason'] = 'disk_budget'
            break
        dest = a.output / 'cells' / f'u{uid:04}'
        log = a.output / 'logs' / f'u{uid:04}.txt'
        code, stopped, elapsed, peak = run_bounded(
            [binary, 'ripen-use', history, str(uid), str(dest), '--max-rounds', str(a.rounds)],
            log, a.timeout, a.rss_mb)
        row = {'use_id': uid, 'template': uses[uid]['template'],
               'seconds': elapsed, 'sampled_peak_rss_bytes': peak}
        if stopped:
            row['status'] = stopped
            # These are this survey's incomplete products, never source histories.
            shutil.rmtree(dest, ignore_errors=True)
        elif code:
            message = log.read_text(errors='replace')
            row.update(status='rejected' if not dest.exists() else 'failed',
                       reason=message.split('Error:')[-1].strip()[:1200])
        else:
            result = json.loads((dest / 'result.json').read_text())
            if result['origin']['members'] != uses[uid]['members'] or result['origin']['template'] != uses[uid]['template']:
                row['status'] = 'identity_mismatch'
            else:
                annotate(result, r, uid)
                for target in [dest / 'result.json', dest / 'run/ripen.json']:
                    target.write_text(json.dumps(result) + '\n')
                row.update(status=result['ripen']['state'], round=result['ripen']['round'],
                           exported=result['saturated_rule_composition']['status'] == 'exported')
                if row['status'] == 'Saturated' and row['exported']:
                    closed.append(str(dest / 'run'))
        summary['attempts'].append(row)
        summary['counts'] = dict(Counter(x['status'] for x in summary['attempts']))
        summary['not_attempted'] = len(uses) - len(summary['attempts'])
        (a.output / 'scan.json').write_text(json.dumps(summary, indent=2) + '\n')
        if (uid + 1) % 25 == 0:
            print(uid + 1, summary['counts'], flush=True)
    catalog = a.output / 'catalog'
    if closed:
        subprocess.run([binary, 'saturated-rule-composition-catalog', str(catalog), *closed], check=True,
                       stdout=subprocess.DEVNULL)
        data = json.loads((catalog / 'catalog.json').read_text())
    else:
        (catalog / 'states').mkdir(parents=True)
        data = dict(schema='saturated-rule-composition-catalog/v1', saturated_rule_compositions=0, triggers=[],
                    comparisons=[], comb_groups=[], state_groups=[], unresolved_comparisons=0)
        (catalog / 'catalog.dot').write_text('digraph Empty {}\n')
    summary['status'] = 'complete' if len(summary['attempts']) == len(uses) else 'partial'
    summary['exported_saturated'] = len(closed)
    summary['saturated_rule_composition_templates'] = len({x['template'] for x in summary['attempts'] if x['status']=='Saturated' and x.get('exported')})
    summary['saturated_rule_compositions'] = data['saturated_rule_compositions']
    summary['shared_instance_states'] = sum(g['trigger_count'] > 1 for g in data['state_groups'])
    summary['shared_template_states'] = sum(g['template_count'] > 1 for g in data['state_groups'])
    summary['unresolved_comparisons'] = data['unresolved_comparisons']
    (a.output / 'scan.json').write_text(json.dumps(summary, indent=2) + '\n')
    data['scan'] = summary
    (catalog / 'catalog.json').write_text(json.dumps(data, indent=2) + '\n')
    print(json.dumps({k: v for k, v in summary.items() if k != 'attempts'}, indent=2))


if __name__ == '__main__':
    main()

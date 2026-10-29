#!/usr/bin/env python3
"""Profile one captured Run with Apple's gpudebug; retain and reuse its session.

Results are diagnostic replay measurements, not live end-to-end timings.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess


def json_records(text):
    decoder = json.JSONDecoder()
    records = []
    i = 0
    while i < len(text):
        if text[i] not in '{[':
            i += 1
            continue
        try:
            value, size = decoder.raw_decode(text[i:])
        except ValueError:
            i += 1
            continue
        records.append(value)
        i += size
    return records


def duration_ns(text):
    m = re.fullmatch(r'([\d.]+)\s*(ns|µs|us|ms|s)', text)
    if not m:
        return None
    return float(m[1]) * {'ns': 1, 'µs': 1000, 'us': 1000, 'ms': 1e6, 's': 1e9}[m[2]]


def invoke(args, commands, path, timeout=300):
    try:
        # File output preserves partial logs and avoids an inherited daemon pipe
        # keeping communicate() alive after the CLI has exited.
        with path.open('w') as log:
            p = subprocess.run(['xcrun', 'gpudebug', *args, '--json'],
                               input='\n'.join(commands)+'\n', stdout=log,
                               stderr=subprocess.STDOUT, text=True, timeout=timeout)
    except BaseException:
        if '--gputrace' in args:
            for own in re.findall(r'^Session (\d+) created\.$', path.read_text(), re.M):
                subprocess.run(['xcrun','gpudebug','--terminate',own],capture_output=True)
        raise
    text = path.read_text()
    if p.returncode or re.search(r'(^|> )error:', text, re.M):
        if '--gputrace' in args:
            for own in re.findall(r'^Session (\d+) created\.$', text, re.M):
                subprocess.run(['xcrun','gpudebug','--terminate',own],capture_output=True)
        raise RuntimeError('gpudebug failed; inspect ' + str(path))
    return text


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--trace', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--reuse', action='store_true', help='load embedded profiling instead of collecting again')
    p.add_argument('--top', type=int, default=3, help='inspect longest encoders, by duration (not Cost percent)')
    p.add_argument('--timeout-seconds', type=int, default=300, help='explicit Apple replay/compilation timeout; not an inference budget')
    a = p.parse_args()
    if a.timeout_seconds <= 0:p.error('--timeout-seconds must be positive')
    a.output.mkdir(parents=True, exist_ok=False)
    session = None
    try:
        # Xcode 27.1 leaves the collecting session's performance browser empty
        # even after `profile load` says "already loaded". Reopen the embedded
        # result in a fresh session, without repeating GPU collection.
        if not a.reuse:
            invoke(['--gputrace', str(a.trace.resolve())], [
                'profile run --gpu-state default --exec overlapping',
                'profile embed', 'exit --terminate'], a.output/'collection.txt', a.timeout_seconds)
        initial = invoke(['--gputrace', str(a.trace.resolve())], ['profile load',
            'go performance', 'go encoders', 'list --all',
            'go ..', 'go shaders', 'list --all', 'exit'], a.output/'overview.txt', a.timeout_seconds)
        ids = re.findall(r'^Session (\d+) created\.$', initial, re.M)
        if not ids:
            raise RuntimeError('gpudebug did not expose a session ID')
        session = ids[-1]
        records = json_records(initial)
        encoders = []; shaders = []
        for r in records:
            if not isinstance(r, dict) or not r.get('children'):
                continue
            children = r['children']
            if all(re.fullmatch(r'(ce|be|re)\d+', x.get('name', '')) for x in children):
                encoders = children
            if all(re.fullmatch(r'comp\d+', x.get('name', '')) for x in children):
                shaders = children
        def elapsed(e):
            values = [duration_ns(str(v.get('value', ''))) for v in e.get('values', []) if isinstance(v, dict)]
            return next((x for x in reversed(values) if x is not None), -1)
        selected = sorted(encoders, key=lambda e: -elapsed(e))[:a.top]
        if not selected or any(elapsed(x) < 0 for x in selected):
            raise RuntimeError('encoder duration data missing; cannot rank hotspots')
        commands = ['go ..', 'go encoders']
        functions = set()
        for e in selected:
            functions.update(re.findall(r'fn=(\w+)', json.dumps(e)))
            commands += ['go '+e['name'], 'info --all', 'go counters']
            for category in ['performance_limiters', 'memory', 'compute_kernel']:
                commands += ['go '+category, 'info --all', 'list --all', 'go ..']
            commands += ['go ..', 'go ..']
        commands += ['go ..', 'go shaders']
        for s in shaders:
            if s['values'][0]['value'] in functions:
                commands += ['info '+s['name']]
        commands += ['exit --terminate']
        details = invoke(['--session', session], commands, a.output/'details.txt', a.timeout_seconds)
        result = {'diagnostic_replay_only': True, 'trace': str(a.trace.resolve()),
                  'reused_embedded_profile': a.reuse, 'selected_encoders': selected,
                  'overview': records, 'details': json_records(details),
                  'limits': ['Cost percentage is not elapsed-time share.',
                             'Shader profiling may sample a subset of cores; preserve the reported core count.',
                             'Live timing, profiled replay duration and static shader properties are distinct.']}
        (a.output/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
        print(a.output/'summary.json')
    finally:
        if session:
            # Only the session this command created. Never terminate all sessions.
            subprocess.run(['xcrun', 'gpudebug', '--terminate', session], capture_output=True)


if __name__ == '__main__':
    main()

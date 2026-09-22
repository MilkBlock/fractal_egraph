#!/usr/bin/env python3
"""Run examples end to end and record which ones actually produce shared states.

Parsing (classify_examples.py) is necessary but not sufficient: the upstream egg math demo
parses fine and saturates nothing, because every ripen job is rejected. Only a real run answers
"is this example worth offering", and that is what this measures.

Each output directory is deleted as soon as it has been read, so a sweep does not accumulate
gigabytes of round snapshots.

  python3 tools/egglog_debugger/verify_examples.py --candidates name,name,... [--timeout 180]
  python3 tools/egglog_debugger/verify_examples.py --all            # every parseable example
"""
import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from server import ROOT, DEFAULT_DEMO, merged_examples  # noqa: E402

OUT = Path(__file__).resolve().parent / 'example-verified.json'


def measure(binary, name, source, work, timeout, rounds=None):
    """Run one example. Returns (states, rounds, detail); states is None when it did not run."""
    folder = work / f'run-{abs(hash(name)):x}'
    egg = work / f'{abs(hash(name)):x}.egg'
    egg.write_text(source)
    shutil.rmtree(folder, ignore_errors=True)
    command = [str(binary), 'analyze', '--recapture-tier0', '--source', str(egg),
               '--output', str(folder)]
    if rounds:
        # Only valid for a simple (run N) schedule, but it keeps the big demos bounded.
        command += ['--rounds', str(rounds)]
    try:
        done = subprocess.run(command, capture_output=True, text=True, cwd=ROOT, timeout=timeout)
    except subprocess.TimeoutExpired:
        shutil.rmtree(folder, ignore_errors=True)
        return None, None, f'timeout after {timeout}s'
    rounds = sorted(folder.glob('rounds/round-*.json'))
    states = rounds and None
    detail = ''
    if not rounds:
        detail = (done.stderr or done.stdout).strip().splitlines()[-1][:160] if (done.stderr or done.stdout) else f'exit {done.returncode}'
    else:
        try:
            snapshot = json.loads(rounds[-1].read_text()).get('saturated_rule_composition') or {}
            catalog = (snapshot.get('catalog') or {}).get('catalog') or {}
            states = catalog.get('saturated_rule_compositions')
            counts = snapshot.get('counts') or {}
            detail = ', '.join(f'{k} {v}' for k, v in sorted(counts.items()))
        except (OSError, ValueError) as error:
            detail = f'unreadable snapshot: {error}'
    shutil.rmtree(folder, ignore_errors=True)
    return states, len(rounds), detail


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/egg_layout')
    parser.add_argument('--demo', type=Path, default=DEFAULT_DEMO)
    parser.add_argument('--candidates', help='comma-separated example names')
    parser.add_argument('--all', action='store_true', help='every parseable example')
    parser.add_argument('--timeout', type=float, default=180)
    parser.add_argument('--rounds', type=int, help='override a simple (run N) schedule')
    parser.add_argument('--output', type=Path, default=OUT)
    args = parser.parse_args()

    examples = merged_examples(args.demo)  # already filtered to what parses
    if args.all:
        names = sorted(examples)
    else:
        names = [n.strip() for n in (args.candidates or '').split(',') if n.strip()]
    if not names:
        parser.error('pass --candidates or --all')

    verified, failed = [], {}
    with tempfile.TemporaryDirectory(prefix='verify-examples-') as tmp:
        work = Path(tmp)
        for name in names:
            if name not in examples:
                failed[name] = 'not in the served (parseable) list'
                print(f'  SKIP {name}  <- not parseable here')
                continue
            states, rounds, detail = measure(args.binary, name, examples[name], work, args.timeout, args.rounds)
            if states:
                verified.append({'name': name, 'shared_states': states, 'rounds': rounds, 'counts': detail})
                print(f'  OK   {name:<34} shared_states={states} rounds={rounds}  ({detail})')
            else:
                failed[name] = detail
                print(f'  FAIL {name:<34} {detail[:80]}')

    # Keep earlier verified entries that were not part of this sweep.
    previous, rejected = {}, {}
    try:
        saved = json.loads(args.output.read_text())
        previous = {e['name']: e for e in saved.get('verified', [])}
        rejected = dict(saved.get('not_saturating') or {})
    except (OSError, ValueError):
        pass
    for entry in verified:
        previous[entry['name']] = entry
        rejected.pop(entry['name'], None)   # a later sweep may have moved it into verified
    rejected.update(failed)
    payload = {
        'scope': ('Ran end to end on the pinned kernel and counted shared SaturatedRuleComposition '
                  'states in the final boundary. An entry here is evidence, not a guarantee: it '
                  'depends on the ripening budget, so a larger budget can only add states.'),
        'probed_at': datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
        'verified': sorted(previous.values(), key=lambda e: -e['shared_states']),
        'not_saturating': dict(sorted(rejected.items())),
    }
    args.output.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + '\n')
    print(f"\n{len(verified)} verified this sweep; {len(previous)} total -> {args.output}")
    return 0


if __name__ == '__main__':
    sys.exit(main())

"""Four small native-analyze examples, not a standalone pattern simulator."""
import argparse
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--binary', type=Path, default=ROOT / 'target/release/egg_layout')
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    source = (ROOT / 'experiments/cell_egraph/port-progression.egg').read_text()
    cases = {
        'positive': source,
        'different-closures': '\n'.join(line for line in source.splitlines()
                                      if ':name "pair-to-left"' not in line) + '\n',
        'disconnected': source.replace('(R (Atom 17) (Atom 2))', '(R (Atom 51) (Atom 64))'),
        'branch': source.replace('(run 4)', '(R (Atom 17) (Atom 99))\n(run 4)'),
    }
    summary = {'scope': 'actual native analyze; finite snapshot-local port recognition', 'cases': {}}
    for name, text in cases.items():
        path = args.output / (name + '.egg')
        path.write_text(text)
        dest = args.output / name
        with (args.output / (name + '.log')).open('w') as log:
            subprocess.run([str(args.binary.resolve()), 'analyze', '--source', str(path),
                            '--output', str(dest), '--save-history'], check=True,
                           env={**os.environ, 'EGG_LAYOUT_GRAPHS': '1', 'EGG_LAYOUT_FULL_TRACE': '0'},
                           stdout=log, stderr=log)
        report = json.loads((dest / 'cell-egraph.json').read_text())['repetition_families']['port_progressions']
        d = report['discovery']
        assert d['total_records'] == d['accounted_records'] == sum(d['counts'].values())
        summary['cases'][name] = {key: d[key] for key in ('total_records', 'counts', 'by_reason')}
        print(name, json.dumps(summary['cases'][name], ensure_ascii=False), flush=True)
    (args.output / 'summary.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2) + '\n')


if __name__ == '__main__':
    main()

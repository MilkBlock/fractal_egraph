"""Re-ripen one existing catalog family with current token-to-term provenance."""
import argparse
import json
from pathlib import Path
import subprocess


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('catalog', type=Path)
    p.add_argument('--state', type=int, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--binary', type=Path, default=Path('target/release/egg_layout'))
    p.add_argument('--rounds', type=int, default=4)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=False)
    catalog = json.loads(a.catalog.read_text())
    triggers = [t for t in catalog['triggers'] if t['saturated_rule_composition'] == a.state]
    if not triggers:
        raise ValueError('empty family')
    runs = []
    for i, t in enumerate(triggers):
        origin = t['origin']
        dest = a.output / f'cell-{i:04}'
        result = subprocess.run([str(a.binary), 'ripen-use', origin['history'],
            str(origin['use_id']), str(dest), '--max-rounds', str(a.rounds)],
            capture_output=True, text=True, timeout=15)
        if result.returncode:
            raise RuntimeError(result.stderr[-2000:])
        fresh = json.loads((dest/'origin.json').read_text())
        if fresh['members'] != origin['members']:
            raise ValueError('Use membership drifted; regenerate survey before comparing')
        if not (dest/'run'/'saturated-rule-composition.json').exists():
            raise ValueError('re-ripen did not export a Saturated state')
        runs.append(str(dest/'run'))
    subprocess.run([str(a.binary), 'saturated-rule-composition-catalog', str(a.output/'catalog'), *runs],
                   check=True, timeout=30)
    print(f'Rebuilt {len(runs)} triggers. Inspect new catalog state IDs before measuring.')


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Same-binary RipSatCell state-machine ablation; offline Bake and checks untimed."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/bench'))
from math_virtual import measure
from math_graph_equal import verify

VARIANTS = {
    'frozen': (0, 0, 0, 0),
    'batch': (0, 1, 0, 0),
    'committed': (1, 1, 0, 0),
    'staged': (1, 1, 1, 0),
    'expanded_machine': (1, 1, 1, 1),
    'machine': (1, 1, 1, 1),
}

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', required=True)
    p.add_argument('--rounds', nargs='+', type=int, default=[6, 8, 10])
    p.add_argument('--runs', type=int, default=3)
    a = p.parse_args()
    if a.runs < 1 or any(n < 1 for n in a.rounds): p.error('positive runs/rounds required')
    os.chdir(ROOT)
    out = Path(a.output).resolve(); out.mkdir(parents=True, exist_ok=False)
    binary = ROOT / 'target/release/examples/adaptive_math'
    library = ROOT / 'experiments/math_mini3/adaptive_runtime/bake_lib_round2.json'
    source = (ROOT / 'experiments/math_mini3/source.egg').read_text()
    assert source.count('(run 3)') == 1
    for key in ('EGG_LAYOUT_CELL_TRANSITIONS', 'EGG_LAYOUT_QUERY_PROFILE'):
        os.environ.pop(key, None)
    os.environ.update(RAYON_NUM_THREADS='1', EGG_LAYOUT_RUNTIME_SUMMARY='1',
        EGG_LAYOUT_AUDIT_ENCLOSURES='0', EGG_LAYOUT_SKIP_INTERNAL_RHS='0',
        EGG_LAYOUT_PRUNE_INTERNAL_JOIN='0', EGG_LAYOUT_CELL_EVOLUTION='0')
    report = dict(schema='ripsat-cell-machine-bench/v1',
        commit=subprocess.check_output(['git','rev-parse','HEAD'], text=True).strip(),
        dirty=bool(subprocess.check_output(['git','status','--porcelain'])),
        binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
        library=dict(path=str(library.relative_to(ROOT)), bytes=library.stat().st_size,
                     sha256=hashlib.sha256(library.read_bytes()).hexdigest()),
        notes=['actual egglog runtime, frozen positive-constructor Bake; not a standalone simulation',
               'one Rayon thread; same reporting mode and source in every variant',
               'staged/queued are promises, not committed effects',
               'native reference may reach a smaller graph in the same number of rounds',
               'verification runs separate; offline Bake and compilation excluded',
               'internal seconds include source parsing/library loading, installation, queries, packing; final stdout serialization/drop excluded',
               'process timing/RSS additionally includes reporting and teardown; macOS wall time has 0.01s resolution',
               'external measurement guard only: 30 seconds or 4 GiB per process'],
        cases=[])
    for rounds in a.rounds:
        case_dir = out / str(rounds); case_dir.mkdir()
        src = case_dir / 'source.egg'; src.write_text(source.replace('(run 3)', f'(run {rounds})'))
        case = dict(rounds=rounds, variants={}, checks={})
        def run(name, suffix, check=False):
            mode = 'native' if name == 'native' else 'frozen'
            flags = VARIANTS.get(name, (0,0,0,0))
            for k,v in zip(('CELL_MACHINE','BATCH_CELLS','CELL_PROMISES','CELL_ORBITS'), flags):
                os.environ['EGG_LAYOUT_'+k] = str(v)
            os.environ['EGG_LAYOUT_CELL_COMPACT_STATES'] = str(int(name == 'machine'))
            os.environ['EGG_LAYOUT_AUDIT_VERIFY'] = str(int(check))
            dest = case_dir / f'{name}-{suffix}'
            cmd = [binary, mode, src, dest]
            if mode == 'frozen': cmd.append(library)
            return measure(cmd, case_dir / f'{name}-{suffix}-process')
        for name in VARIANTS:
            run(name, 'check', True)
            case['checks'][name] = verify(case_dir/'frozen-check/verification-graph.json',
                                          case_dir/f'{name}-check/verification-graph.json')
        # Interleave variants and reverse the order to reduce systematic warm-up bias.
        for i in range(a.runs):
            names = [*VARIANTS, 'native']
            if i % 2: names.reverse()
            for name in names:
                case['variants'].setdefault(name, []).append(run(name, str(i)))
        case['medians'] = {}
        for name, runs in case['variants'].items():
            m = {k: statistics.median(r['result'][k] for r in runs)
                 for k in ('seconds','setup_seconds','execution_seconds','packing_seconds','matches')}
            m.update(peak_rss_bytes=statistics.median(r['peak_rss_bytes'] for r in runs),
                     process_wall_seconds=statistics.median(r['process_wall_seconds'] for r in runs),
                     final=runs[0]['result']['final'])
            m['stored_state_rows'] = (runs[0]['result'].get('cell_states') or {}).get('stored_state_rows', 0)
            for key in ('completed_prefixes', 'promised_prefixes', 'queued_prefixes', 'unresolved_completion_plans'):
                m[key] = statistics.median(sum(s[key] for s in r['result']['samples']) for r in runs)
            m['shared_storage'] = {k:v for k,v in runs[0]['result']['storage'].items() if k != 'tables'}
            case['medians'][name] = m
        report['cases'].append(case)
        (out/'measurements.json').write_text(json.dumps(report, indent=2)+'\n')
        print(rounds, {n: (round(v['seconds']*1000,2),v['matches']) for n,v in case['medians'].items()}, flush=True)
    # Keep a compact, independently inspectable text result under version control.
    compact = {k:v for k,v in report.items() if k!='cases'}
    compact['cases'] = [{k:v for k,v in c.items() if k!='variants'} for c in report['cases']]
    (out/'summary.json').write_text(json.dumps(compact, indent=2)+'\n')

if __name__ == '__main__': main()

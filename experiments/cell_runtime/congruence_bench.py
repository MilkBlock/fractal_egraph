"""One same-binary off/on pair; complete states checked, no averaging.

Uses the existing CPU measurement adapter. Real Pass-3 controls retain their
known native discrepancies; this experiment only claims equality to off.
"""
import argparse
from collections import Counter
import json
import os
from pathlib import Path
import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from eggfuse.engine.cell_runtime import binary, prepare
from eggfuse.ir.cell_runtime import build
from experiments.cell_runtime.congruence_cases import chain
from experiments.cell_runtime.suite import timed

def main():
    p = argparse.ArgumentParser()
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--engine', type=Path)
    p.add_argument('--real-controls', type=Path)
    a = p.parse_args(); a.output.mkdir(parents=True, exist_ok=False)
    engine = a.engine or binary()
    cases = {}
    for name, depth, instances in [('wide',8,256), ('deep',32,256)]:
        src = a.output/(name+'.egg'); src.write_text(chain(depth, instances))
        cases[name] = build(prepare(src))
    if a.real_controls:
        for name in ('fact','loopfact','sum-bits'):
            old = json.loads((a.real_controls/name/'program.json').read_text())
            cases[name] = build(old['source_core'])
    summary = {}
    previous = os.environ.get('CELL_CPU_CONGRUENCE')
    try:
        for name, program in cases.items():
            path = a.output/(name+'.json'); path.write_text(json.dumps(program))
            runs = []
            for flag in ('0','1'):
                os.environ['CELL_CPU_CONGRUENCE'] = flag
                runs.append(timed(path, engine, a.output/(name+'-'+flag)))
            states = [json.loads((a.output/(name+'-'+flag)/'state.json').read_text()) for flag in ('0','1')]
            assert states[0] == states[1], name+' full graph differs'
            reports = [r[1] for r in runs]
            keys = ('ordinal','ruleset','path','iterations','epoch','logical_matches','changed','live_cells')
            assert [[s[k] for k in keys] for s in reports[0]['steps']] == [[s[k] for k in keys] for s in reports[1]['steps']], name+' schedule differs'
            entry = {'measurements':[r[0] for r in runs], 'full_graph_and_schedule_equal':True,
                     'inference_speedup':runs[0][0]['inference_seconds']/runs[1][0]['inference_seconds'],
                     'local_congruence':reports[1]['local_congruence'], 'stats':[r['stats'] for r in reports]}
            entry['phases'] = []
            for report in reports:
                phases = Counter()
                for step in report['steps']: phases.update(step.get('phases',{}))
                entry['phases'].append(dict(phases))
            summary[name] = entry
            (a.output/'results.json').write_text(json.dumps(summary, indent=2)+'\n')
            print(name, json.dumps({k:entry[k] for k in ('inference_speedup','local_congruence','phases')}), flush=True)
    finally:
        if previous is None: os.environ.pop('CELL_CPU_CONGRUENCE',None)
        else: os.environ['CELL_CPU_CONGRUENCE'] = previous

if __name__ == '__main__': main()

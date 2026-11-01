"""Measure compiler-produced Cell CPU artifacts, never rebuild a raw source_core.

Compile with `python3 -m eggfuse compile --backend cell-cpu` first. Each
--pipeline NAME=DIR must include the complete SK/marker/device pass chain.
Low-level synthetic correctness fixtures remain in check.py, not this benchmark.
"""
import argparse
from collections import Counter
import json
import os
from pathlib import Path
import subprocess
import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from eggfuse.engine.cell_runtime import binary,load_pipeline_artifact
from experiments.cell_runtime.suite import timed

def eligible(provenance, capabilities):
    if not provenance['selected_choices']:
        raise ValueError('structural-key pass selected zero choices; this is not an SK-enabled benchmark input')
    if not provenance['marker_count']:
        raise ValueError('marker pass emitted zero markers; this is not a marker-enabled benchmark input')
    if not capabilities.get('validated') or not capabilities.get('congruence_reorder_safe'):
        raise ValueError('CPU engine has not certified this artifact for local congruence; no fallback timing is allowed')
    if not capabilities.get('congruence_templates'):
        raise ValueError('artifact contains no local congruence programs')

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--engine', type=Path)
    p.add_argument('--pipeline', action='append', required=True, metavar='NAME=COMPILE_DIR')
    a = p.parse_args()
    cases = {}
    for spec in a.pipeline:
        name,sep,folder = spec.partition('=')
        if not sep or not name or Path(name).name!=name or name in cases:
            p.error('--pipeline requires a unique NAME=COMPILE_DIR')
        cases[name] = load_pipeline_artifact(folder)
    engine = a.engine or binary()
    # Validate the actual backend before running either timed lane. Unsupported
    # SK primitives or merge contracts fail here, not after a misleading control.
    for path,_,provenance in cases.values():
        proc=subprocess.run([str(engine),'--program',str(path),'--inspect-program'],capture_output=True,text=True)
        if proc.returncode:raise ValueError('CPU artifact preflight failed: '+proc.stderr.strip())
        eligible(provenance,json.loads(proc.stdout))
    a.output.mkdir(parents=True, exist_ok=False)
    summary = {}; previous = os.environ.get('CELL_CPU_CONGRUENCE')
    try:
        for name,(path,_,provenance) in cases.items():
            runs = []
            for flag in ('0','1'):
                os.environ['CELL_CPU_CONGRUENCE'] = flag
                runs.append(timed(path, engine, a.output/(name+'-'+flag)))
            states = [json.loads((a.output/(name+'-'+flag)/'state.json').read_text()) for flag in ('0','1')]
            assert states[0] == states[1], name+' full graph differs'
            reports = [r[1] for r in runs]
            assert reports[1]['local_congruence']['enabled'], 'candidate silently disabled its optimization'
            keys = ('ordinal','ruleset','path','iterations','epoch','logical_matches','changed','live_cells')
            assert [[s[k] for k in keys] for s in reports[0]['steps']] == [[s[k] for k in keys] for s in reports[1]['steps']], name+' schedule differs'
            entry = {'compiler_provenance':provenance,'measurements':[r[0] for r in runs],
                     'full_graph_and_schedule_equal':True,'inference_speedup':runs[0][0]['inference_seconds']/runs[1][0]['inference_seconds'],
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

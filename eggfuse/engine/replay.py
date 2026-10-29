"""Suspend/resume a saved resident program without replaying its prefix."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def replay(program, build, output, resume=None, stop_after_runs=None, quantum=None):
    sys.path.insert(0, str(ROOT / 'experiments/device_backend'))
    from run_generic_cells import build_worker
    program, output = Path(program).resolve(), Path(output).resolve()
    payload = json.loads(program.read_text())
    if payload.get('generic_cell', {}).get('checkpoint_version') != 1:
        raise ValueError('recompile the resident program for checkpoint ABI v1')
    if quantum is not None:
        if quantum <= 0:
            raise ValueError('quantum must be positive')
        payload['quantum'] = quantum
    if stop_after_runs is not None and not 0 <= stop_after_runs <= 0xffffffff:
        raise ValueError('stop-after-runs is an absolute nonnegative u32 Run count')
    output.mkdir(parents=True, exist_ok=False)
    saved = output / 'program.json'
    saved.write_text(json.dumps(payload))
    worker = build_worker(Path(build).resolve())
    command = [str(worker), str(saved), str(output / 'result.json')]
    if resume is not None:
        command += ['--resume', str(Path(resume).resolve())]
    if stop_after_runs is not None:
        command += ['--stop-after-runs', str(stop_after_runs), '--checkpoint', str(output / 'checkpoint')]
    with (output / 'worker.log').open('w') as log:
        subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
    result = json.loads((output / 'result.json').read_text())
    if any(s['meta'][4] for s in result['instances']):
        raise ValueError('GPU execution failed; see result.json')
    summary = {key: result[key] for key in ('paused', 'restored_runs', 'inference_seconds', 'gpu_seconds',
                                           'shader_compile_seconds', 'checkpoint_seconds',
                                           'program_sha256', 'worker_sha256')}
    summary.update(complete=all(s['completed'] for s in result['instances']),
                   completed_runs=[len(s['history']) for s in result['instances']],
                   checkpoint=str(output / 'checkpoint') if stop_after_runs is not None else None,
                   timing_scope='resumed suffix only; excludes shader compilation, checkpoint IO and state export')
    (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    return summary


def main(args):
    p = argparse.ArgumentParser(prog='eggfuse replay', description=__doc__)
    p.add_argument('--program', type=Path, required=True, help='saved program JSON with initial instance frames')
    p.add_argument('--build', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--resume', type=Path)
    p.add_argument('--stop-after-runs', type=int, help='absolute completed Run count per instance, not additional Runs')
    p.add_argument('--quantum', type=int)
    a = p.parse_args(args)
    print(json.dumps(replay(a.program, a.build, a.output, a.resume, a.stop_after_runs, a.quantum), indent=2))
    return 0

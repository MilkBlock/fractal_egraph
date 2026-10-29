#!/usr/bin/env python3
"""Capture exactly one ABI7 resident Run from a physical checkpoint.

No DVB protocol, prefix replay, inference budgets or rule changes. Captured phase
encoders are separate for attribution, so these timings are diagnostic only.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


def sha(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def record(repo, program, checkpoint, build, output, reference=None):
    sys.path.insert(0, str(repo))
    from eggfuse.engine.replay import replay
    manifest=json.loads((checkpoint/'manifest.json').read_text())
    if manifest['schema']!='eggfuse-resident-checkpoint/v1' or manifest['instances']!=1:
        raise ValueError('requires one resident arena checkpoint')
    first=manifest['completed_runs'][0];output.mkdir(parents=True,exist_ok=False)
    # This sets the child process temporary directory too; Apple tools can have
    # other system-owned caches, so it is not a promise to relocate every cache.
    temporary=output/'tmp';temporary.mkdir()
    previous=os.environ.get('TMPDIR');os.environ['TMPDIR']=str(temporary)+'/'
    try:
        result=replay(program,build,output/'captured',checkpoint,first+1,quantum=1,
                      capture=output/'run.gputrace')
        if reference is None:
            replay(program,build,output/'reference',checkpoint,first+1,quantum=1)
            reference=output/'reference/result.json'
        a=json.loads((output/'captured/result.json').read_text())
        b=json.loads(reference.read_text())
        for field in ['program_sha256','worker_sha256','restored_runs','instances']:
            if a[field]!=b[field]:raise ValueError('capture changed resident state: '+field)
        if len(a['instances'][0]['history'])!=first+1:
            raise ValueError('capture did not execute exactly one Run')
        trace=output/'run.gputrace'
        if not trace.exists() or not any(p.is_file() for p in trace.rglob('*')):
            raise ValueError('empty or missing Metal capture')
        provenance=dict(schema='eggfuse-resident-profile/v1',
            scope='actual resident ABI7 single-Run capture; not a benchmark',
            program=str(program),program_file_sha256=sha(program),
            checkpoint=str(checkpoint),checkpoint_manifest_sha256=sha(checkpoint/'manifest.json'),
            checkpoint_program_sha256=manifest['program_sha256'],worker_sha256=result['worker_sha256'],
            start_after=first,end_run=first+1,reference=str(reference),
            full_state_and_history_equal=True,encoder_mode='separate phases during capture only',
            xcode=subprocess.check_output(['xcodebuild','-version'],text=True).strip(),
            child_tmpdir=str(temporary),disk_free_after=shutil.disk_usage(output).free,
            temp_limit='TMPDIR redirects child temporary files, not all Apple system caches')
        (output/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
        print(json.dumps(provenance,indent=2))
        return provenance
    finally:
        if previous is None:os.environ.pop('TMPDIR',None)
        else:os.environ['TMPDIR']=previous


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--repo',type=Path,required=True)
    p.add_argument('--program',type=Path,required=True)
    p.add_argument('--checkpoint',type=Path,required=True)
    p.add_argument('--build',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--reference',type=Path,help='reuse an existing identical one-Run replay result')
    a=p.parse_args()
    record(a.repo.resolve(),a.program.resolve(),a.checkpoint.resolve(),a.build.resolve(),a.output.resolve(),a.reference.resolve() if a.reference else None)


if __name__=='__main__':main()

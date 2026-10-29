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


def timings(repo, program, checkpoint, build, output):
    """Split host command buffers for GPU phase timestamps; no shader edits."""
    sys.path[:0]=[str(repo),str(repo/'experiments/device_backend')]
    from run_generic_cells import build_worker
    from eggfuse.engine.replay import replay
    base=build_worker(build);base_hash=sha(base)
    manifest=json.loads((checkpoint/'manifest.json').read_text())
    if manifest['instances']!=1 or manifest['worker_sha256']!=base_hash:
        raise ValueError('timing adapter requires a checkpoint from this exact base worker')
    source=repo/'engine/metal/generic_cell.swift'
    from eggfuse.engine.scheduled import worker_source
    text=worker_source()
    if hashlib.sha256(text.encode()).hexdigest()!=(build/'generic-cell-worker.sha256').read_text():
        raise ValueError('base worker source fingerprint differs')
    def replace(before,after):
        nonlocal text
        if text.count(before)!=1:raise ValueError('resident timing adapter anchor changed: '+before[:80])
        text=text.replace(before,after)
    replace('let workerDigest=digest(try Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[0]).resolvingSymlinksInPath()))',
        'let actualProfileWorkerDigest=digest(try Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[0]).resolvingSymlinksInPath()))\nlet workerDigest="'+base_hash+'"')
    replace('let quantum=UInt32(program["quantum"] as? Int ?? 8)',
        'let quantum=UInt32(1)\nvar profileTotals=[String:Double](),profileCounts=[String:Int](),profileSlices=[(String,MTLCommandBuffer)](),profileName=""')
    replace(' let cb=queue.makeCommandBuffer()!\n cb.label=transportVersion', ' profileSlices.removeAll(keepingCapacity:true)\n var cb=queue.makeCommandBuffer()!\n cb.label=transportVersion')
    replace('if capturing && phaseStarted {e.endEncoding();e=cb.makeComputeCommandEncoder()!;bind(e)}',
        'if phaseStarted {e.endEncoding();cb.commit();profileSlices.append((profileName,cb));cb=queue.makeCommandBuffer()!;e=cb.makeComputeCommandEncoder()!;bind(e)}\n    profileName=name;cb.label=name')
    replace('gpuSeconds+=cb.gpuEndTime-cb.gpuStartTime;dispatches+=1',
        '''profileSlices.append((profileName,cb))
 for (name,part) in profileSlices {
  try require(part.status == .completed,"profile phase failed")
  let elapsed=part.gpuEndTime-part.gpuStartTime;gpuSeconds+=elapsed
  profileTotals[name,default:0]+=elapsed;profileCounts[name,default:0]+=1
 };dispatches+=profileSlices.count''')
    replace('"profile_capture":capturing,',
        '"profile_capture":capturing,"profile_phase_seconds":profileTotals,"profile_phase_calls":profileCounts,"profile_worker_sha256":actualProfileWorkerDigest,"profile_kind":"separate command buffers; diagnostic only",')
    output.mkdir(parents=True,exist_ok=False);instrumented=output/'generic-profile.swift';instrumented.write_text(text)
    worker=output/'worker'
    subprocess.run(['xcrun','swiftc','-O',str(instrumented),'-o',str(worker)],check=True)
    first=manifest['completed_runs'][0]
    env={**os.environ,'EGGFUSE_METAL_CACHE':str(build/'metal-cache')}
    with (output/'worker.log').open('w') as log:
        subprocess.run([str(worker),str(program),str(output/'result.json'),'--resume',str(checkpoint),
            '--stop-after-runs',str(first+1),'--checkpoint',str(output/'diagnostic-checkpoint')],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
    # The diagnostic checkpoint is not offered as a base-worker checkpoint.
    cp=output/'diagnostic-checkpoint/manifest.json';cm=json.loads(cp.read_text())
    cm['worker_sha256']=sha(worker);cm['instrumented_base_worker_sha256']=base_hash
    cp.write_text(json.dumps(cm,indent=2)+'\n')
    replay(program,build,output/'reference',checkpoint,first+1,quantum=1)
    a=json.loads((output/'result.json').read_text());b=json.loads((output/'reference/result.json').read_text())
    for key in ['program_sha256','restored_runs','instances']:
        if a[key]!=b[key]:raise ValueError('phase timing changed execution: '+key)
    result=dict(schema='eggfuse-resident-phase-timings/v1',scope='actual GPU phase timestamps; separate command buffers perturb scheduling, not a benchmark',
        program=str(program),program_sha256=a['program_sha256'],checkpoint=str(checkpoint),start_after=first,end_run=first+1,
        base_worker_sha256=base_hash,instrumented_worker_sha256=sha(worker),instrumented_source_sha256=sha(instrumented),
        phase_seconds=a['profile_phase_seconds'],phase_calls=a['profile_phase_calls'],
        instrumented_gpu_seconds=a['gpu_seconds'],reference_gpu_seconds=b['gpu_seconds'],
        full_state_and_history_equal=True,shader_compile_seconds=a['shader_compile_seconds'],pipeline_cache=a['pipeline_cache'])
    (output/'timings.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2));return result


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
    p.add_argument('--mode',choices=['capture','timings'],default='capture')
    a=p.parse_args()
    if a.mode=='timings':
        if a.reference:p.error('timings mode validates its own same-worker reference')
        timings(a.repo.resolve(),a.program.resolve(),a.checkpoint.resolve(),a.build.resolve(),a.output.resolve())
    else:record(a.repo.resolve(),a.program.resolve(),a.checkpoint.resolve(),a.build.resolve(),a.output.resolve(),a.reference.resolve() if a.reference else None)


if __name__=='__main__':main()

#!/usr/bin/env python3
"""Profile full resident execution without disabling private schedule regions."""
import argparse,hashlib,json,os,re,subprocess,sys
from pathlib import Path

HERE=Path(__file__).resolve().parent

def sha(p):
    with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()

def replace(text,before,after):
    if text.count(before)!=1:raise ValueError('resident profiler anchor differs: '+before[:90])
    return text.replace(before,after)

def instrument(text,base_hash):
    # Only host instrumentation. No mutation of program JSON, control, shader,
    # quantum, private region eligibility, match/effect data or identities.
    text=text.replace('queue.makeCommandBuffer()!','pfCommand()')
    text=re.sub(r'\b(\w+)\.waitUntilCompleted\(\)',r'pfWait(\1)',text)
    text=re.sub(r'\b(\w+)\.commit\(\)',r'pfCommit(\1)',text)
    text=replace(text,'var e=cb.makeComputeCommandEncoder()!;bind(e);var phaseStarted=false','var e:MTLComputeCommandEncoder! = nil;var phaseStarted=false')
    text=replace(text,'if capturing && phaseStarted {e.endEncoding();e=cb.makeComputeCommandEncoder()!;bind(e)}','if let previous=e {previous.endEncoding()};e=pfEncoder(cb,name);bind(e)')
    text=replace(text,'let e=cb.makeComputeCommandEncoder()!,pipeline=smPipelines[index]','let e=pfEncoder(cb,"private_"+smNames[index]),pipeline=smPipelines[index]')
    text=replace(text,'"profile_capture":capturing,','"profile_capture":capturing,"full_encoder_profile":pfReport(),')
    anchor='let device=MTLCreateSystemDefaultDevice()!,queue=device.makeCommandQueue()!'
    text=replace(text,anchor,anchor+'\n'+(HERE/'resident_metrics.swift').read_text().replace('__BASE_WORKER_HASH__',base_hash))
    return text

def verify(a,b):
    for k in ['program_sha256','restored_runs','instances','private_schedule_execution','private_schedule_regions','private_schedule_runs','private_schedule_rows','private_schedule_forwarded_reads','private_schedule_binding_outputs','dispatches']:
        if a.get(k)!=b.get(k):raise ValueError('instrumentation changed '+k)
    if not all(s['completed'] and s['meta'][4]==0 for s in a['instances']):raise ValueError('incomplete profiled program')
    return dict(full_state_and_history_equal=True,private_schedule_execution=a['private_schedule_execution'],private_schedule_regions=a['private_schedule_regions'],private_schedule_runs=a['private_schedule_runs'],outer_command_buffers=a['dispatches'])

def category(name):
    if name.startswith('private_'):return 'private_schedule'
    if 'merge_row_index' in name or name=='resident_merge_index_gate':return 'merge_index_refresh'
    if 'rebuild' in name or name.startswith('resident_merge_') or 'compact_' in name:return 'rebuild'
    if name.startswith('resident_query_') or name.startswith('resident_row_index_') or name in ['batched_cell_prepare','batched_cell_prepare_finish']:return 'prepare_indexes_control'
    if name.startswith('resident_seed_') or name=='batched_cell_match':return 'seed_and_match'
    if 'sort_' in name or name.startswith('batched_cell_pack'):return 'sort_pack'
    if name=='batched_cell_evaluate':return 'evaluate'
    if name.startswith('cell_union_'):return 'union'
    if name.startswith('cell_wave_'):return 'effect_waves'
    if name.startswith('resident_batch_') or name.startswith('resident_write_') or name.startswith('resident_packet_') or name=='batched_cell_publish':return 'rhs_publish'
    if 'fingerprint' in name:return 'audit_gates'
    return 'unclassified'

def summarize(a,b,program=None):
    p=a['full_encoder_profile'];groups={}
    for name,v in p['phases'].items():
        g=groups.setdefault(category(name),dict(seconds=0.,issued_encoders=0))
        g['seconds']+=v['seconds'];g['issued_encoders']+=v['issued_encoders']
    commands=p['commands'];gpu=sum(c['gpu_seconds'] for c in commands);covered=sum(c['encoder_interval_union_seconds'] for c in commands)
    duration_sum=sum(v['seconds'] for v in p['phases'].values())
    if p['pending_commands']:raise ValueError('unresolved profile command buffers')
    if len(commands)<a['dispatches']:raise ValueError('outer command buffers missing from profile')
    if duration_sum+1e-6<covered:raise ValueError('encoder interval union exceeds summed duration')
    if covered>gpu*1.05+0.001:raise ValueError('timestamp units/accounting inconsistent with GPU command time')
    work=None
    if all(len(s.get('batch_statistics',[]))>=60 for s in b['instances']):
        # Reuse the engine's counter decoder, record its identity; do not invent
        # profiler-specific meanings for matches, requests and fresh rows.
        sys.path.insert(0,str(HERE.parents[2]))
        from eggfuse.engine.regions import execution_summary
        work=dict(decoder_sha256=sha(HERE.parents[2]/'eggfuse/engine/regions.py'),values=execution_summary(dict(programs=[dict(instances=b['instances'],attempts=[b])])),storage_bytes=b.get('storage_bytes'),final_rows=sum(s['meta'][0] for s in b['instances']))
    configuration=None if program is None else dict(backend='resident ABI7',quantum=program.get('quantum',8),source_marker_program_present=program.get('marker_program') is not None,graph_instances=len(program['instances']),ordinary_rhs=program.get('resident_ordinary_rhs'))
    return dict(schema='eggfuse-resident-full-profile/v1',verification=verify(a,b),configuration=configuration,work=work,reference_inference_seconds=b['inference_seconds'],profiled_inference_seconds=a['inference_seconds'],timing_perturbation_ratio=a['inference_seconds']/b['inference_seconds'],
      registered_command_buffers=len(commands),private_or_other_command_buffers=len(commands)-a['dispatches'],all_command_gpu_seconds=gpu,encoder_interval_union_seconds=covered,encoder_duration_sum_seconds=duration_sum,overlapping_encoder_seconds=duration_sum-covered,unattributed_command_seconds=gpu-covered,groups=groups,
      phases=p['phases'],cpu_wait_calls=p['cpu_wait_calls'],cpu_wait_wall_seconds=p['cpu_wait_wall_seconds'],cpu_command_encoding_wall_seconds=p['cpu_command_encoding_wall_seconds'],counter_resolution_wall_seconds=p['counter_resolution_wall_seconds'],
      hot_commands=sorted(commands,key=lambda c:-c['gpu_seconds'])[:20],
      limits=['Encoder sampling/splitting perturbs timing; group shares describe this diagnostic, not an exact decomposition of the clean run.','Issued encoders can be zero-work indirect dispatches; they are not active kernel launches.','Command time minus covered encoder intervals is not pure barrier or CPU time.','CPU waits include GPU work and cannot be added to GPU seconds.','Private and ordinary execution paths and all result/history fields are checked against the frozen control.'])

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--run',type=Path,required=True,help='saved attempt directory with program.json and result.json');p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    run=a.run.resolve();build=a.build.resolve();out=a.output.resolve();reference=json.loads((run/'result.json').read_text())
    base=build/'generic-cell-worker';source=build/'generic-cell-worker.swift';fingerprint=sha(base)
    if reference['worker_sha256']!=fingerprint:raise ValueError('saved control and base worker differ')
    if sha(source)!=(build/'generic-cell-worker.sha256').read_text():raise ValueError('materialized worker source fingerprint differs')
    out.mkdir(parents=True,exist_ok=False);text=instrument(source.read_text(),fingerprint);profile_source=out/'worker.swift';profile_source.write_text(text);worker=out/'worker'
    subprocess.run(['xcrun','swiftc','-O',str(profile_source),'-o',str(worker)],check=True)
    provenance=dict(program=str(run/'program.json'),program_file_sha256=sha(run/'program.json'),reference=str(run/'result.json'),reference_sha256=sha(run/'result.json'),base_worker_sha256=fingerprint,profile_worker_sha256=sha(worker),profile_source_sha256=sha(profile_source),xcode=subprocess.check_output(['xcodebuild','-version'],text=True).strip())
    (out/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
    env={**os.environ,'EGGFUSE_METAL_CACHE':str(build/'metal-cache')}
    try:
        with (out/'worker.log').open('w') as log:subprocess.run([str(worker),str(run/'program.json'),str(out/'result.json')],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        actual=json.loads((out/'result.json').read_text());result=summarize(actual,reference,json.loads((run/'program.json').read_text()))
    except BaseException as error:
        (out/'failure.json').write_text(json.dumps(dict(error=str(error),diagnostic_only=True),indent=2)+'\n')
        raise
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k not in ['phases','hot_commands','limits']},indent=2))
if __name__=='__main__':main()

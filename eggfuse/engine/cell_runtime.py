"""Separate-process Cell execution. Native processes only prepare or audit."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time
import os

from eggfuse.ir.cell_runtime import build, validate
ROOT=Path(__file__).resolve().parents[2]

def binary():
    subprocess.run(['cargo','build','--offline','--release','--manifest-path',str(ROOT/'engine/cell-runtime/Cargo.toml')],check=True)
    return ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime'

def frontend_binary():
    p=ROOT/'target/debug/examples/eggfuse_frontend'
    if not p.exists():
        subprocess.run(['cargo','build','--offline','--example','eggfuse_frontend'],cwd=ROOT,check=True)
    return p

def prepare(source, frontend=None):
    p=subprocess.run([str(frontend or frontend_binary()),str(source.resolve())],input='',capture_output=True,text=True,check=False)
    if p.returncode:raise ValueError('Native frontend rejected the source: '+p.stderr[-4000:])
    return json.loads(p.stdout)

def execute(program,out,engine=None,audit=False,checkpoint_after=None,resume=None,extract=None,save_checkpoint=False,environment=None):
    validate(program)
    out.mkdir(parents=True,exist_ok=False)
    path=out/'cell-runtime-program.json';path.write_text(json.dumps(program))
    cmd=[str(engine or binary()),'--program',str(path),'--output',str(out/'runtime')]
    if audit:cmd.append('--audit-stages')
    if save_checkpoint:cmd.append('--save-checkpoint')
    if checkpoint_after is not None:cmd+=['--checkpoint-after',str(checkpoint_after)]
    if resume is not None:cmd+=['--resume',str(resume.resolve())]
    if extract is not None:cmd+=['--extract-unit-cost',extract]
    with (out/'runtime.log').open('w') as f:subprocess.run(cmd,stdout=f,stderr=subprocess.STDOUT,check=True,env={**os.environ,**(environment or {})})
    return json.loads((out/'runtime/report.json').read_text())

def run_artifact(artifact,output,check=False,engine=None):
    from eggfuse.compiler.marker_ir import validate as validate_markers
    artifact=Path(artifact);output=Path(output)
    manifest=json.loads((artifact/'manifest.json').read_text())
    if manifest.get('schema')!='eggfuse-device-artifact/v1' or manifest.get('backend')!='cell-cpu':raise ValueError('not a Cell CPU artifact')
    required={'program.json','core.json','recipe-program.json','marker-program.json','source_sk_mk.egg','capabilities.json'}
    if not required<=manifest['files'].keys():raise ValueError('incomplete Cell artifact')
    for name,checksum in manifest['files'].items():
        if Path(name).name!=name or hashlib.sha256((artifact/name).read_bytes()).hexdigest()!=checksum:raise ValueError('Cell artifact checksum mismatch: '+name)
    program=json.loads((artifact/'program.json').read_text());core=validate(program)
    if 'multi_stage' in program:
        if 'multi-stage-program.json' not in manifest['files'] or json.loads((artifact/'multi-stage-program.json').read_text())!=program['multi_stage']:raise ValueError('multi-stage artifact differs')
    if 'congruence' in program:
        if 'local-congruence-program.json' not in manifest['files'] or json.loads((artifact/'local-congruence-program.json').read_text())!=program['congruence']:raise ValueError('congruence artifact differs')
    if core!=json.loads((artifact/'core.json').read_text()) or program['recipe']!=json.loads((artifact/'recipe-program.json').read_text()):raise ValueError('Cell artifact program/source mismatch')
    validate_markers(json.loads((artifact/'marker-program.json').read_text()),core)
    source_sha=hashlib.sha256((artifact/'source_sk_mk.egg').read_bytes()).hexdigest()
    if source_sha!=manifest['source_sha256']:raise ValueError('marked source differs')
    engine=engine or binary();report=execute(program,output,engine,audit=check)
    result={'backend':'cell-cpu','complete_schedule':report['status']=='complete','inference_seconds':report['inference_seconds'],'checked':check,'report':report}
    if check:
        from experiments.cell_runtime.audit import check as audit
        result['binding_audit']=audit(program,output/'runtime')
        execute(program,output/'replay',engine,audit=True)
        for name in ('state.json','stages.json'):
            if json.loads((output/'runtime'/name).read_text())!=json.loads((output/'replay/runtime'/name).read_text()):raise ValueError('Cell fresh-process replay differs: '+name)
        result['reproducible']=True
        result['verification_scope']='complete-LHS audit and CPU Cell fresh-process state/round replay; no claim of native full endpoint equivalence'
    return result

def main(args):
    p=argparse.ArgumentParser(prog='eggfuse cell')
    source=p.add_mutually_exclusive_group(required=True);source.add_argument('--source',type=Path);source.add_argument('--program',type=Path)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--frontend',type=Path);p.add_argument('--cell-bin',type=Path);p.add_argument('--audit-stages',action='store_true');p.add_argument('--check',action='store_true')
    p.add_argument('--checkpoint-after',type=int);p.add_argument('--resume',type=Path);p.add_argument('--extract-unit-cost');p.add_argument('--save-checkpoint',action='store_true')
    a=p.parse_args(args);start=time.perf_counter()
    program=json.loads(a.program.read_text()) if a.program else build(prepare(a.source,a.frontend))
    if a.check and a.checkpoint_after is not None:raise ValueError('full native endpoint comparison requires completed execution')
    report=execute(program,a.output,a.cell_bin,a.audit_stages,a.checkpoint_after,a.resume,a.extract_unit_cost,a.save_checkpoint)
    if a.source:
        (a.output/'source.egg').write_bytes(a.source.read_bytes())
        report['source_sha256']=hashlib.sha256(a.source.read_bytes()).hexdigest()
    if a.check:
        if not a.source:raise ValueError('--check requires source for independent native execution')
        from .cpu import binary as native_binary,compare_native
        native=a.output/'native';cmd=[str(native_binary()),'--source',str(a.source.resolve()),'--mode','native','--output',str(native.resolve())]
        with (a.output/'native.log').open('w') as f:subprocess.run(cmd,stdout=f,stderr=subprocess.STDOUT,check=True)
        actual=json.loads((a.output/'runtime/state.json').read_text());reference=json.loads((native/'state.json').read_text())
        assertions=set(program['source_core'].get('assertion_tables',[]));actual['rows']=[r for r in actual['rows'] if r['table'] not in assertions]
        report['verification']=compare_native(program['source_core'],actual,reference)
    report['driver_seconds']=time.perf_counter()-start
    (a.output/'report.json').write_text(json.dumps(report,indent=2))
    print(json.dumps({k:v for k,v in report.items() if k!='steps'},indent=2));return 0

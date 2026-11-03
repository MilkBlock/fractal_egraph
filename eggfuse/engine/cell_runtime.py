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

def load_artifact(artifact):
    from eggfuse.compiler.marker_ir import validate as validate_markers
    artifact=Path(artifact)
    manifest=json.loads((artifact/'manifest.json').read_text())
    if manifest.get('schema')!='eggfuse-device-artifact/v1' or manifest.get('backend')!='cell-cpu':raise ValueError('not a Cell CPU artifact')
    required={'program.json','core.json','recipe-program.json','marker-program.json','source_sk_mk.egg','capabilities.json'}
    if not required<=manifest['files'].keys():raise ValueError('incomplete Cell artifact')
    for name,checksum in manifest['files'].items():
        if Path(name).name!=name or hashlib.sha256((artifact/name).read_bytes()).hexdigest()!=checksum:raise ValueError('Cell artifact checksum mismatch: '+name)
    program=json.loads((artifact/'program.json').read_text());core=validate(program)
    if 'reference_views' in program:
        if 'reference-views.json' not in manifest['files'] or json.loads((artifact/'reference-views.json').read_text())!=program['reference_views']:raise ValueError('reference views differ')
    if 'compositions' in program:
        if 'composition-program.json' not in manifest['files'] or json.loads((artifact/'composition-program.json').read_text())!=program['compositions']:raise ValueError('composition recipes differ')
    if 'merge_contracts' in program:
        if 'merge-contracts.json' not in manifest['files'] or json.loads((artifact/'merge-contracts.json').read_text())!=program['merge_contracts']:raise ValueError('merge contracts differ')
    if 'merge_programs' in program:
        if 'merge-programs.json' not in manifest['files'] or json.loads((artifact/'merge-programs.json').read_text())!=program['merge_programs']:raise ValueError('merge programs differ')
    if 'known_results' in program:
        if 'known-results.json' not in manifest['files'] or json.loads((artifact/'known-results.json').read_text())!=program['known_results']:raise ValueError('known results differ')
    if 'entry_schedule' in program:
        if 'entry-schedule.json' not in manifest['files'] or json.loads((artifact/'entry-schedule.json').read_text())!=program['entry_schedule']:raise ValueError('entry schedule artifact differs')
    if 'multi_stage' in program:
        if 'multi-stage-program.json' not in manifest['files'] or json.loads((artifact/'multi-stage-program.json').read_text())!=program['multi_stage']:raise ValueError('multi-stage artifact differs')
    if 'congruence' in program:
        if 'local-congruence-program.json' not in manifest['files'] or json.loads((artifact/'local-congruence-program.json').read_text())!=program['congruence']:raise ValueError('congruence artifact differs')
    if core!=json.loads((artifact/'core.json').read_text()) or program['recipe']!=json.loads((artifact/'recipe-program.json').read_text()):raise ValueError('Cell artifact program/source mismatch')
    marker=json.loads((artifact/'marker-program.json').read_text());validate_markers(marker,core)
    if 'marker_program_sha256' in program:
        if program['marker_program_sha256']!=marker['sha256'] or program['reference_views']['marker_tables']!=sorted(m['name'] for m in marker['markers']):raise ValueError('reference marker contract differs')
    source_sha=hashlib.sha256((artifact/'source_sk_mk.egg').read_bytes()).hexdigest()
    if source_sha!=manifest['source_sha256']:raise ValueError('marked source differs')
    return program

def load_pipeline_artifact(output):
    """Benchmark entry: source -> SK -> markers -> typed recipe -> CPU artifact.

    A completed but empty SK/marker pass is reported as such, not as an enabled
    optimization. Kernel fixtures may still use execute() for independent tests.
    """
    from eggfuse.compiler.passes import verified_receipts,digest
    root=Path(output).resolve();artifact=root/'eggfuse_related_files'
    required=['structural_keys','analyze','markers','marker_ir','recipe_ir','device']
    if (root/'pipeline.json').is_file() and any(s['name']=='bake_environment' for s in json.loads((root/'pipeline.json').read_text()).get('passes',[])):
        required.append('bake_environment')
    records=verified_receipts(root,required)
    from eggfuse.analyzer.health import validate as validate_bake_health
    bake_health=validate_bake_health(records['analyze'][0]/'result')
    has_compositions=(artifact/'composition-program.json').is_file()
    if has_compositions:records.update(verified_receipts(root,['compositions']))
    device,receipt=records['device']
    if receipt['contract']['options'].get('backend')!='cell-cpu':raise ValueError('compiler output is not cell-cpu')
    for rel,checksum in receipt['artifacts'].items():
        if digest(artifact/rel)!=checksum:raise ValueError('published CPU artifact differs from device pass: '+rel)
    links=(('structural_keys','result/source_sk.egg','markers','source_sk'),
           ('analyze','result/cell-recipes.json','markers','analysis'),
           ('analyze','result/cell-recipes.json','marker_ir','analysis'),
           ('markers','result/published.egg','marker_ir','marked_source'),
           ('marker_ir','core.json','recipe_ir','core'),
           ('marker_ir','core.json','device','core'),
           ('marker_ir','marker-program.json','device','marker'),
           ('recipe_ir','recipe-program.json','device','recipe'),
           ('markers','result/published.egg','device','source'))
    for producer,rel,consumer,input_name in links:
        if records[producer][1]['artifacts'][rel]!=records[consumer][1]['contract']['inputs'][input_name]['sha256']:raise ValueError('compiler pass wiring differs: '+producer+' -> '+consumer)
    if has_compositions:
        for producer,rel,consumer,key in [('recipe_ir','recipe-program.json','compositions','recipe'),('marker_ir','marker-program.json','compositions','marker'),('compositions','composition-program.json','device','compositions')]:
            if records[producer][1]['artifacts'][rel]!=records[consumer][1]['contract']['inputs'][key]['sha256']:raise ValueError('composition pass wiring differs')
    analyzed=records['analyze'][1]['contract']['inputs']['source_sk']['sha256']
    if analyzed!=records['structural_keys'][1]['artifacts']['result/source_sk.egg']:
        environment=records.get('bake_environment')
        if not environment or environment[1]['contract']['inputs']['bake_source']['sha256']!=analyzed or environment[1]['contract']['inputs']['source_sk']['sha256']!=records['structural_keys'][1]['artifacts']['result/source_sk.egg']:
            raise ValueError('Bake input lacks a validated matching rule environment')
    program=load_artifact(artifact)
    sk=json.loads((records['structural_keys'][0]/'result/report.json').read_text())
    marker=json.loads((artifact/'marker-program.json').read_text())
    provenance={'pipeline':str(root),'passes':{n:r[1]['contract'] for n,r in records.items()},
                'bake_health':bake_health,
                'selected_choices':sk.get('selected_choices',[]),'marker_count':len(marker['markers']),
                'source_sha256':records['structural_keys'][1]['contract']['inputs']['source']['sha256'],
                'source_sk_sha256':records['structural_keys'][1]['artifacts']['result/source_sk.egg'],
                'source_sk_mk_sha256':digest(artifact/'source_sk_mk.egg'),'program_sha256':digest(artifact/'program.json')}
    return artifact/'program.json',program,provenance

def run_artifact(artifact,output,check=False,engine=None):
    output=Path(output);program=load_artifact(artifact)
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

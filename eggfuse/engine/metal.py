"""Stable Metal entry; compiled artifacts never invoke a second lowering."""
import argparse,hashlib,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
BACKEND=ROOT/'experiments/device_backend'

def run_artifact(artifact,build,output,check=False):
    artifact=Path(artifact).resolve();build=Path(build).resolve();output=Path(output).resolve()
    manifest=json.loads((artifact/'manifest.json').read_text())
    if manifest.get('schema')!='eggfuse-device-artifact/v1' or manifest.get('backend')!='gpu':raise ValueError('run --artifact currently requires an ordinary GPU artifact')
    for name,sha in manifest['files'].items():
        if Path(name).name!=name:raise ValueError('invalid artifact path')
        if hashlib.sha256((artifact/name).read_bytes()).hexdigest()!=sha:raise ValueError('artifact checksum mismatch: '+name)
    required={'program.json','core.json','kernel.metal','marker-program.json','recipe-program.json','source_sk_mk.egg'}
    if not required<=manifest['files'].keys():raise ValueError('artifact is not self-contained; recompile with the current compiler')
    options=manifest.get('execution',{})
    if set(options)!={'stage_contexts','joint_polyspine','block_merge','matrix_saturate'} or any(type(v) is not bool for v in options.values()):raise ValueError('invalid execution options')
    program=json.loads((artifact/'program.json').read_text())
    if str(BACKEND) not in sys.path:sys.path.insert(0,str(BACKEND))
    from run import run
    return run(artifact/'source_sk_mk.egg',build,output,check_every=0,check_at_end=check,
        capacity=program['capacity'],match_capacity=program['match_capacity'],save_final_state=True,
        marker_program=artifact/'marker-program.json',compiled_program=artifact/'program.json',
        cell_program=artifact/'multi-stage-cell.json' if 'multi-stage-cell.json' in manifest['files'] else None,**options)

def main(args,library=False):
    if not library and ('--artifact' in args or args in (['--help'],['-h'])):
        p=argparse.ArgumentParser(prog='eggfuse run');p.add_argument('--artifact',type=Path,required=True);p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--check',action='store_true');a=p.parse_args(args)
        r=run_artifact(a.artifact,a.build,a.output,a.check)
        print(json.dumps({k:r[k] for k in ['complete_schedule','inference_seconds','checked']},indent=2));return 0
    runner=BACKEND/('run_generic_cells.py' if library else 'run.py')
    return subprocess.run([sys.executable,str(runner),*args],cwd=ROOT).returncode

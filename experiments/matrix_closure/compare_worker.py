"""One matrix/direct pair; reuse checked original facts and the native control."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

HERE=Path(__file__).resolve().parent

def compare(saved,worker,out):
    report=json.loads((saved/'report.json').read_text())
    if not report['full_graph_check']['exact_rows_values_equivalence_visibility'] or not all(c['exact_predicates'] for c in report['per_run_checks']):
        raise ValueError('reference is not completely verified')
    if hashlib.sha256(Path(report['source']).read_bytes()).hexdigest()!=report['source_sha256']:
        raise ValueError('native source changed')
    out.mkdir(parents=True,exist_ok=False)
    expected=(saved/'birth.bin').read_bytes();results={}
    for mode in ['matrix','direct']:
        path=out/mode;path.mkdir()
        env=dict(os.environ,MATRIX_CLOSURE_MODE=mode)
        if env.get('MTL_DEBUG_LAYER')=='1':raise ValueError('timing excludes Metal validation')
        with (path/'worker.log').open('w') as log:
            subprocess.run([str(worker.resolve()),str((saved/'program.json').resolve()),str(HERE/'kernel.metal'),str(path/'birth.bin'),str(path/'gpu.json')],env=env,check=True,stdout=log,stderr=log)
        if (path/'birth.bin').read_bytes()!=expected:raise AssertionError(mode+' changed a fact or its visibility round')
        results[mode]=json.loads((path/'gpu.json').read_text())
    result={'scope':'same frozen native closure, original schedule frontiers; not full Pass 3',
            'reference':str(saved),'source_sha256':report['source_sha256'],
            'program_sha256':hashlib.sha256((saved/'program.json').read_bytes()).hexdigest(),
            'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),
            'kernel_sha256':hashlib.sha256((HERE/'kernel.metal').read_bytes()).hexdigest(),
            'native_control_reused':report['native_timing_control'],
            'operator_preparation_seconds':report['operator_preparation_seconds'],
            'full_graph_check_reused':report['full_graph_check'],
            'all_fact_birth_times_identical':True,'lanes':results}
    (out/'report.json').write_text(json.dumps(result,indent=2)+'\n')
    return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--reference',type=Path,required=True)
    p.add_argument('--worker',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();print(json.dumps(compare(a.reference,a.worker,a.output),indent=2))

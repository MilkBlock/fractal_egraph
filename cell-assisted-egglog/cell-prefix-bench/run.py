#!/usr/bin/env python3
"""Original eggcc If rules, held-out contexts, frozen analyze Cell catalog.
One run per lane. Use --mode check or profile in a separate output directory.
"""
import argparse, hashlib, json, os, subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--analysis',type=Path,required=True)
p.add_argument('--worker',type=Path,required=True)
p.add_argument('--output',type=Path,required=True)
p.add_argument('--instances',type=int,default=300)
p.add_argument('--mode',choices=['time','check','profile'],default='time')
p.add_argument('--skip-build',action='store_true')
a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
assert a.instances>0
source=ROOT/'experiments/cell_spine/eggcc_if_tiny/source.egg'
text=source.read_text();shapes=[l for l in text.splitlines() if l.startswith('(If ')]
assert len(shapes)==3
env='\n'.join(l for l in text.splitlines() if not l.startswith('(If ') and not l.startswith('(run'))
inputs=[]
for i in range(a.instances):
    kind=(i//10)%2 if i%10==0 else 2
    inputs.append(shapes[kind].replace(['train-min','train-max','train-miss'][kind],f'cell-prefix-heldout-{i}'))
program=out/'heldout.egg';program.write_text(env+'\n'+'\n'.join(inputs)+'\n(run-schedule (saturate if-tiny))\n')
if not a.skip_build:
    subprocess.run(['cargo','build','--offline','--release','--example','cell_prefix_bench','--features','native-read-profile','--target-dir','target/analyzer'],cwd=ROOT,check=True)
binary=ROOT/'target/analyzer/release/examples/cell_prefix_bench'
command=[str(binary),str(program),str(a.analysis.resolve()),str(a.worker.resolve()),str(out/'raw.json')]
if a.mode!='time':command.append(a.mode)
with (out/'run.log').open('w') as log:
    subprocess.run(command,cwd=ROOT,env=dict(os.environ,RAYON_NUM_THREADS='1'),stdout=log,stderr=log,check=True)
r=json.loads((out/'raw.json').read_text())
sha=lambda f:hashlib.sha256(Path(f).read_bytes()).hexdigest()
files=[source,binary,a.worker.resolve(),program,*sorted((a.analysis/'catalog').rglob('*.json'))]
files += [*sorted((ROOT/'cell-assisted-egglog/src').glob('*.rs')),ROOT/'cell-assisted-egglog/matrix.swift',ROOT/'cell-assisted-egglog/family.metal',ROOT/'cell-assisted-egglog/family_cells.metal',Path(__file__).resolve(),ROOT/'examples/cell_prefix_bench.rs',ROOT/'examples/support/live_table_bench.rs']
r['provenance']={'hashes':{str(f):sha(f) for f in files},'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'instances':a.instances,'original_rules':3,'scope':'Original isolated eggcc If/util rules and held-out inputs; not whole raytrace or compiler. Frozen analyzed library, not a new bake.'}
for v in r['variants']:
    assert v['verification']['exact_rows_values_equivalence_visibility']
    if a.mode=='check':assert sum(q['stats']['audit_queries'] for q in v['report']['rules'])>0
(out/'results.json').write_text(json.dumps(r,indent=2)+'\n')
print('CPU',r['cpu']['runtime']['run_seconds'])
for v in r['variants']:
    x=v['report'];print(v['name'],x['runtime']['run_seconds'],'plans',len(x['cells']['plans']),x['cell_work'])
print(out/'results.json')

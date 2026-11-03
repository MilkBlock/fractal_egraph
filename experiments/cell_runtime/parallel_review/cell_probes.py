"""Run isolated diagnostics. Time only the frozen entry projection in the replay."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
from experiments.cell_runtime.suite import ROOT

HERE=Path(__file__).resolve().parent

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('raw',type=Path)
    ap.add_argument('reference',type=Path)
    a=ap.parse_args()
    raw=a.raw.resolve()
    probe=raw/'probe'
    engine=probe/'engine/cell-runtime'
    target=engine/'target/release'
    manifest=engine/'Cargo.toml'
    plan=json.loads(a.reference.read_text())
    program=plan['program']
    config=plan['lanes']['on']['measurement']['configuration']
    (probe/'engine').mkdir(parents=True,exist_ok=False)
    for crate in ('cell-runtime','structural-key'):
        subprocess.run(['cp','-cR',ROOT/'engine'/crate,probe/'engine'],check=True)
    def apply(name):
        subprocess.run(['patch','-p1','-i',HERE/name],cwd=probe,check=True)
    def build(*args):
        subprocess.run(['cargo','build','--release','--offline','--manifest-path',manifest,'--target-dir',engine/'target',*args],check=True)
    def run(name,extra):
        d=raw/name
        subprocess.run([target/'eggfuse-cell-runtime','--program',program,'--output',d],
                       env={**os.environ,**config,**extra},stdout=subprocess.DEVNULL,check=True)
        ref=raw/('plain-join/1-both/state.json' if name=='span-lean' else 'cell-matrix/1-nodes/state.json')
        assert json.loads((d/'state.json').read_text())==json.loads(ref.read_text())
        return json.loads((d/'report.json').read_text())
    apply('cell-span.patch')
    build('--bin','eggfuse-cell-runtime')
    for name,flag in [('nodes','0'),('both','1')]:
        run('span-'+name,{'CELL_CPU_STAGE_SPINE':flag,'CELL_CPU_PROFILE':'1'})
    run('span-lean',{**json.loads((raw/'plain-join/1-both/measurement.json').read_text())['configuration'],'CELL_CPU_PROFILE':'1'})
    apply('freeze-entries.patch')
    manifest.write_text(manifest.read_text().replace('[dependencies]','[dependencies]\nrayon = "=1.11.0"'))
    shutil.copy(HERE/'entry_replay.rs',engine/'examples')
    build('--bin','eggfuse-cell-runtime','--example','entry_replay')
    jobs=run('frozen',{'CELL_CPU_STAGE_SPINE':'0'})['entry_schedule']['frozen_entry_jobs']
    (raw/'jobs.json').write_text(json.dumps(jobs,separators=(',',':')))
    cases=[(0,1,'caller'),(4,1,'caller'),(16,1,'caller'),(1,1,'caller'),
           (4,1,'resident'),(16,1,'resident'),(4,64,'resident'),(16,64,'resident')]
    outputs=[]
    for n,cutoff,mode in cases+list(reversed(cases)):
        outputs.append(json.loads(subprocess.check_output([target/'examples/entry_replay',
                        raw/'jobs.json',str(n),str(cutoff),mode])))
    (raw/'replay.json').write_text(json.dumps(outputs,indent=2))

if __name__=='__main__':
    main()

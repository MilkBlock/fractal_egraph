"""Build two isolated drivers and run all six permutations of 1/4/16 workers on macOS."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import shutil
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
REV = 'ae80f82a403c1209fb084b115ee89470c7e66d0e'
ORDERS = [(1,4,16),(4,16,1),(16,1,4),(16,4,1),(4,1,16),(1,16,4)]

def check(args, **kwargs):
    subprocess.run(args, check=True, **kwargs)

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('raw', type=Path)
    ap.add_argument('production', type=Path)
    ap.add_argument('skill', type=Path, help='eggcc-pass-benchmark/scripts/bench.py')
    ap.add_argument('driver_patch',type=Path,help='driver.patch emitted by the existing schedule profiler build')
    args = ap.parse_args()
    if hashlib.sha256(args.driver_patch.read_bytes()).hexdigest() != 'c20a544002b1dce465703187480e21e138acb784de1ff088d0b19ff71559418d':
        raise ValueError('unexpected schedule driver patch')
    raw = args.raw.resolve()
    production = args.production.resolve()
    raw.mkdir(parents=True, exist_ok=False)
    clone = raw/'eggcc'
    check(['git','clone','--local','--shared',production,clone])
    check(['git','checkout','--detach',REV],cwd=clone)
    check(['cp','-cR',production/'target',clone/'target'])
    exe=clone/'target/release/eggcc'
    source=clone/'benchmarks/passing/bril/core/sum-bits.bril'
    clean=[]
    for mode in ('schedule','pool'):
        patches=[HERE/'eggcc-pool.patch']
        if mode=='schedule':
            patches.insert(0,args.driver_patch.resolve())
        for patch in patches:
            check(['git','apply',patch],cwd=clone)
        env=dict(os.environ,LLVM_SYS_180_PREFIX='/opt/homebrew/opt/llvm@18',
                 LIBRARY_PATH='/opt/homebrew/opt/cbc/lib:/opt/homebrew/opt/zstd/lib:/opt/homebrew/lib')
        check(['cargo','build','--offline','--locked','--release','--target-dir',clone/'target','--bin','eggcc'],cwd=clone,env=env)
        if mode=='schedule':
            shutil.copy2(exe,raw/'eggcc-schedule')
            shutil.copy2(exe.with_name('tiger'),raw/'tiger')
        for block,order in enumerate(ORDERS,1):
            for width in order:
                env=dict(os.environ,EGGCC_PROFILE_THREADS=str(width),RAYON_NUM_THREADS=str(width))
                if mode=='schedule':
                    d=raw/f'b{block}-t{width}'
                    check([sys.executable,args.skill,'saturate-run','--production-repo',clone,
                           '--profile-binary',exe,'--source',source,'--output',d],env=env)
                    log=d/'sum-bits/profile.log'
                else:
                    d=raw/f'clean-b{block}-t{width}'
                    d.mkdir()
                    log=d/'stderr'
                    with (d/'program.txt').open('wb') as out,log.open('wb') as err:
                        prev=resource.getrusage(resource.RUSAGE_CHILDREN)
                        start=time.perf_counter()
                        check([exe,source,'--run-mode','optimized-pretty-print','--eggcc-schedule','parallel',
                               '--run-data-out',d/'run.json'],cwd=clone,env=env,stdout=out,stderr=err)
                        wall=time.perf_counter()-start
                        cur=resource.getrusage(resource.RUSAGE_CHILDREN)
                    clean.append(dict(block=block,threads=width,wall=wall,
                                      cpu=cur.ru_utime+cur.ru_stime-prev.ru_utime-prev.ru_stime,
                                      output=hashlib.sha256((d/'program.txt').read_bytes()).hexdigest()))
                assert f'effective Rayon threads={width} ' in log.read_text()
        for patch in reversed(patches):
            check(['git','apply','--reverse',patch],cwd=clone)
    (raw/'clean.json').write_text(json.dumps(clean,indent=2))

if __name__=='__main__':
    main()

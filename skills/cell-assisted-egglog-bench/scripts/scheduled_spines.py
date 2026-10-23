"""Compile complete source with the original layout domain, then use bench.run."""
import os
from pathlib import Path
import subprocess
import sys


def run(a,repo):
    resolve=lambda p:p if p.is_absolute() else repo/p
    out=resolve(a.output)
    python=resolve(a.solver_python) if a.solver_python else next((p for p in [repo/'out/cell-spine-venv/bin/python',Path('/Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python')] if p.is_file()),Path(sys.executable))
    commands=[]
    if not a.no_build:
        commands.extend([['cargo','build','--offline','--release','--example','lhs_ast'],
                         ['cargo','build','--offline','--release','--target-dir','target/analyzer','--example','scheduled_spine','--example','raytrace_stable_bench']])
    compile=[str(python),'-m','tools.cell_spine.compiler','--mode','scheduled','--source',str(resolve(a.snapshot)),
             '--output',str(out/'program'),'--solver',a.solver,'--skip-build']
    for flag in ['query_selection','query_profile','table_profile']:
        if getattr(a,flag):compile.extend(['--'+flag.replace('_','-'),str(resolve(getattr(a,flag)))])
    commands.append(compile)
    if not a.compile_only:
        if not a.reuse_controls:raise ValueError('--reuse-controls is required unless --compile-only')
        bench=[sys.executable,str(repo/'skills/cell-assisted-egglog-bench/scripts/bench.py'),'--repo',str(repo),
            'run','--all-rules','--joint-sat','--snapshot',str(resolve(a.snapshot)),
            '--boundary-programs',str(out/'program/boundary-programs.json'),
            '--reuse-controls',str(resolve(a.reuse_controls)),'--worker',str(resolve(a.worker)),
            '--output',str(out/'run'),'--no-build','--no-cell-metrics']
        if a.no_extract:bench.append('--no-extract')
        commands.append(bench)
    if a.dry_run:
        for c in commands:print(' '.join(c))
        return 0
    out.mkdir(parents=True,exist_ok=False)
    for c in commands:
        result=subprocess.run(c,cwd=repo,env=dict(os.environ,RAYON_NUM_THREADS='1'))
        if result.returncode:return result.returncode
    return 0

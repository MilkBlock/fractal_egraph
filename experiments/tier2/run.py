"""Reproduce tier-2 experiments from the committed native tier-1 snapshot."""
import os,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];os.chdir(ROOT)
def run(*args):subprocess.run(args,check=True)
def py(script):run(sys.executable,'experiments/tier2/'+script+'.py')
run('cargo','build','--quiet','--bin','tier2_run','--bin','tier2_fixture','--bin','tier2_higher','--bin','tier2_observations','--bin','tier1_source_schema')
py('mine');run('target/debug/tier2_run','experiments/tier2/math.egg','experiments/tier2/math_native.json')
py('higher');run('target/debug/tier2_higher');py('check_higher')
py('fixtures')
for name in ['unit','triple','stride','changing','warmup']:
    p=Path('experiments/tier2/fixtures')/(name+'.egg')
    run('target/debug/tier2_fixture',str(p),str(p.with_suffix('.json')))
    run('target/debug/tier1_source_schema',str(p),str(p.with_suffix('.ast.json')))
py('recurrence_bridge');run('target/debug/tier2_observations')
py('affine');run('target/debug/tier2_run','experiments/tier2/affine.egg','experiments/tier2/affine_native.json');py('render')
run(sys.executable,'-m','unittest','discover','-s','experiments/tier2','-p','test_*.py')
run('cargo','test','--quiet','--test','tier2_native')

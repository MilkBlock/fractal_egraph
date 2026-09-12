"""Run and export the shared-template tier-1 example."""
import os
from pathlib import Path
import subprocess
ROOT=Path(__file__).resolve().parents[2]
env=os.environ.copy()
env['TIER1_EFFECT_REPORT']=str(ROOT/'experiments/tier1_effects/results.json')
subprocess.run(['cargo','test','--test','tier1_effects'],cwd=ROOT,env=env,check=True,timeout=180)
subprocess.run(['python3',str(ROOT/'experiments/tier1_effects/render.py')],cwd=ROOT,check=True)

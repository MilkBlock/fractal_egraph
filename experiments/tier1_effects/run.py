"""Run native tier-1 semantic checks and collect their actual reports."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory() as tmp:
    env=os.environ.copy()
    env['TIER1_EFFECT_REPORT_DIR']=tmp
    subprocess.run(['cargo','test','--test','tier1_effects'],cwd=ROOT,env=env,check=True,timeout=180)
    report={name:json.loads((Path(tmp)/(name+'.json')).read_text()) for name in ['union','redundancy','dynamic']}
    (ROOT/'experiments/tier1_effects/results.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')

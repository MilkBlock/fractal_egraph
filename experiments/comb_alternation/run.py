"""Reproduce native history and compare actual dependency path organizations."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
from analyze import analyze
ROOT=Path(__file__).resolve().parents[2]
OUT=Path(__file__).resolve().parent
source=ROOT/'experiments/annotated_export/math_microbenchmark/source.egg'
subprocess.run(['cargo','build','--release','--bin','combine_profile'],cwd=ROOT,check=True)
with tempfile.TemporaryDirectory() as tmp:
    profile=Path(tmp)/'profile.json'
    subprocess.run(['target/release/combine_profile',str(source.relative_to(ROOT)),str(profile),'6'],cwd=ROOT,check=True,timeout=180)
    raw=json.loads(profile.read_text())
    result=analyze(raw)
    result['source_sha256']=hashlib.sha256(source.read_bytes()).hexdigest()
    result['native_event_count']=len(raw['events'])
    result['native_dependency_roots']=len(raw['witnesses'])
    result['source']=str(source.relative_to(ROOT))
    (OUT/'results.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')

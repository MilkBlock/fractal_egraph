"""Check coverage, completeness, source hashes, frozen endpoints and repeat counts."""
from pathlib import Path
import hashlib,json,tarfile,math,collections
r=Path(__file__).resolve().parents[1]
j=lambda p:json.loads((r/p).read_text())
cases=j('evidence/manifest.json')['cases']
survey={(v['case'],v['pass']):v for v in map(json.loads,(r/'suite.jsonl').read_text().splitlines())}
assert len(cases)==96 and len(survey)==288
with tarfile.open(r/'reproduce/captures.tar.gz') as t:
 for c in cases:
  n=c['source'].split('/benchmarks/passing/')[1]
  for k,p in enumerate(c['passes'],1):
   v=survey[n,k]
   assert v['source_sha256']==c['source_sha256'] and v['native_ms']>0 and v['cell_ms']>0
   raw=f"{c['directory']}/capture/{Path(n).stem}/pass-{k}.egg"
   for path,expected in ((raw,v['raw_sha256']),(p['input'],v['flat_sha256'])):
    assert hashlib.sha256(t.extractfile(path).read()).hexdigest()==expected
   assert v['units'] in (59,64)
pairs=j('paired.json')
bydir={c['directory']:c['source'].split('/benchmarks/passing/')[1] for c in cases}
assert len(pairs)==15
for cid in set(p['directory'] for p in pairs):
 group=[p for p in pairs if p['directory']==cid]
 assert sorted(p['block'] for p in group)==list(range(5))
 assert all(p['state_sha256']==survey[bydir[cid],3]['state_sha256'] for p in group)
assert j('semantic-audit.json')['cell_state_sha256']==survey['bril/core/sum-bits.bril',3]['state_sha256']
assert j('grounded-check.json')['result'][0]=='missing grounded key'
print('Verified 96 workloads, 288 measurements per backend, all input hashes, 15 paired trials and the Cell witness endpoint.')

# The memory follow-up has its own source snapshot and paired controls.
import subprocess,sys
subprocess.run([sys.executable,str(r/"reproduce/render-memory-data.py"),"--check"],check=True)

subprocess.run([sys.executable,str(r/"reproduce/render-performance-data.py"),"--check"],check=True)

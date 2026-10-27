"""Use the existing profile skill; add labels for conditional batch pipelines."""
import argparse,hashlib,json,subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent

def prepare(run,out):
    skill=HERE.parents[1]/'skills/eggfuse-metal-profile/scripts/profile.py'
    subprocess.run(['python3',str(skill),'prepare','--run',str(run),'--source',str(HERE/'worker.swift'),'--output',str(out)],check=True)
    path=out/'worker.swift';source=path.read_text();anchor='var profileNames: [ObjectIdentifier: String] = [:]'
    if source.count(anchor)!=1:raise ValueError('profile name-map ABI changed')
    source=source.replace(anchor,anchor+'\nfor (p,n) in zip(batchPipelines,batchNames) { profileNames[ObjectIdentifier(p)] = n }')
    # The new indirect buffer is distinct from rebuild/context indirect buffers.
    source=source.replace(r'indirect=\(indirect)',r'indirect=\(indirect || batchIndirectOffset != nil)')
    path.write_text(source);subprocess.run(['xcrun','swiftc','-O',str(path),'-o',str(out/'worker')],check=True)
    metadata=json.loads((out/'build.json').read_text());digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
    metadata.update(source_sha256=digest(path),worker_sha256=digest(out/'worker'))
    metadata['modifications']+='; conditional batch pipeline names and accurate indirect-dispatch labels'
    (out/'build.json').write_text(json.dumps(metadata,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--run',required=True,type=Path);p.add_argument('--output',required=True,type=Path);a=p.parse_args();prepare(a.run,a.output)

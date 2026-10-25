"""Inventory real typed rules before GPU compilation; does not run inference."""
import argparse
import collections
import hashlib
import json
import subprocess
from pathlib import Path

def main():
    p=argparse.ArgumentParser()
    p.add_argument('--source',type=Path,required=True)
    p.add_argument('--build',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args()
    data=json.loads(subprocess.check_output([str(a.build.resolve()/'frontend'),str(a.source.resolve()),'--inventory']))
    primitives=collections.Counter()
    errors=collections.Counter()
    for r in data['rules']:
        errors[r['export_error'] or 'exportable']+=1
        for call in r['primitives']:
            primitives[(call['name'],tuple(call['inputs']),call['output'])]+=1
    report={
        'scope':'All installed source rules, not scheduled-rule coverage or runtime speed',
        'source':str(a.source.resolve()),
        'source_sha256':hashlib.sha256(a.source.read_bytes()).hexdigest(),
        'frontend_sha256':hashlib.sha256((a.build/'frontend').read_bytes()).hexdigest(),
        'tables':len(data['tables']), 'rules':len(data['rules']),
        'export_status':dict(errors),
        'merges':dict(collections.Counter(t['merge'] for t in data['tables'] if t['merge'])),
        'primitives':[dict(name=k[0],inputs=k[1],output=k[2],uses=v) for k,v in sorted(primitives.items())],
        'note':'Exportable does not imply GPU compiler support; unsupported operations are still rejected there.',
    }
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
if __name__=='__main__':main()

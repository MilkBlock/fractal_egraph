"""Independent full-graph and production extraction gates for saved program lanes."""
import json
from pathlib import Path
import subprocess
import sys
from program_modes import compare,write,sha

def run(a,repo):
    resolve=lambda p:p.resolve() if p.is_absolute() else (repo/p).resolve()
    paths={'cpu':resolve(a.reference),'gpu':resolve(a.candidate)}
    out=resolve(a.output);out.mkdir(parents=True,exist_ok=False);(out/'graphs').mkdir()
    graphs={};identities={}
    for name,path in paths.items():
        r=json.loads((path/'report.json').read_text())
        if not r.get('complete') or not r.get('snapshot_complete'):raise ValueError('incomplete program lane '+name)
        if r.get('backend')!=name:raise ValueError('reference must be CPU and candidate must be GPU')
        if name=='cpu' and r.get('cell_assistance'):raise ValueError('CPU oracle must have Cell assistance disabled')
        export=path/'extraction-input.json'
        if not export.is_file():raise ValueError('missing extraction export; use program-modes --export-extraction')
        (out/'graphs'/f'{name}.json').symlink_to(export)
        identities[name]=dict(directory=str(path),identity=json.loads((path/'identity.json').read_text()),graph_sha256=sha(path/'graph.json'),extraction_sha256=sha(export))
        graphs[name]=json.loads((path/'graph.json').read_text())
    proof=compare(graphs['cpu'],graphs['gpu'],set(graphs['cpu']['tables']))
    write(out/'report.json',dict(schema='source-program-verification/v1',graph=proof,lanes=identities,extraction_lanes=['cpu','gpu'],passed=False))
    del graphs
    rc=subprocess.run([sys.executable,str(repo/'experiments/raytrace_pass3_gpu/extract.py'),str(out),'--tiger',str(resolve(a.tiger))],cwd=repo).returncode
    quality=json.loads((out/'extraction/quality.json').read_text()) if (out/'extraction/quality.json').exists() else {}
    passed=proof.get('status')=='equal' and rc==0 and quality.get('passed') is True
    write(out/'report.json',dict(schema='source-program-verification/v1',graph=proof,lanes=identities,extraction_lanes=['cpu','gpu'],extraction=quality,passed=passed))
    print(json.dumps(dict(graph=proof,extraction_passed=quality.get('passed'),passed=passed)))
    return 0 if passed else 1

"""Three fact programs, one rule environment and a persistent learned library."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    library=out/'knowledge.json';runs=[]
    for name in ('independent','small','shared'):
        source=Path(__file__).parent/(name+'.egg');folder=out/name
        with (out/(name+'.log')).open('w') as log:
            subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',source,'--output',folder,
                            '--knowledge-library',library,'--skip-build'],cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,check=True)
        report=json.loads((folder/'report.json').read_text());validation=json.loads((folder/'validation.json').read_text())
        families=json.loads((folder/'merge-families.json').read_text())
        runs.append(dict(input=name,source_sha256=report['source_sha256'],knowledge=report['knowledge'],
            phase_seconds=report['phase_seconds'],validation_cases=len(validation['cases']),
            all_equivalent=all(c['comparison']['status']=='equivalent' for c in validation['cases']),
            families=[{k:f[k] for k in ('id','status','support_samples','current_instances','parameters','pattern','definition_nodes','encoded_nodes','explicit_nodes')}
                      for f in families['families']]))
    if not runs[1]['knowledge']['imported_cells']:raise ValueError('old Cells did not participate in the new compilation')
    if not any(f['status']=='generalized-hypothesis' and f['support_samples']>=2 for f in runs[-1]['families']):
        raise ValueError('no cross-input merge hypothesis learned')
    result=dict(scope='actual native Cell compilation and revalidation; learning describes witnessed transitions only; no speedup or induction claim',
                library_bytes=library.stat().st_size,runs=runs)
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__=='__main__':main()

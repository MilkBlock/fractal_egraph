"""Build the matrix worker and existing native oracle offline."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

HERE=Path(__file__).resolve().parent
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True)
    p.add_argument('--target',type=Path,required=True);a=p.parse_args()
    a.output.mkdir(parents=True,exist_ok=True)
    subprocess.run([sys.executable,str(HERE.parent/'device_backend/build.py'),'--release',
                    '--output',str(a.output/'oracle'),'--target',str(a.target)],check=True)
    subprocess.run(['xcrun','swiftc','-O',str(HERE/'worker.swift'),'-o',str(a.output/'worker')],check=True)
    sha=lambda path:hashlib.sha256(path.read_bytes()).hexdigest()
    data={'scope':'standalone matrix closure with real native typed rules',
          'sources':{str(f):sha(f) for f in [HERE/'compiler.py',HERE/'kernel.metal',HERE/'worker.swift',HERE/'run.py']},
          'worker_sha256':sha(a.output/'worker'),'oracle':json.loads((a.output/'oracle/build.json').read_text())}
    (a.output/'build.json').write_text(json.dumps(data,indent=2)+'\n')

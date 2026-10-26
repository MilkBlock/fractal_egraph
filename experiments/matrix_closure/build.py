"""Build static/resident matrix workers and the native oracle offline."""
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
    for source, binary in [('worker.swift','worker'),('dynamic_worker.swift','dynamic-worker')]:
        subprocess.run(['xcrun','swiftc','-O',str(HERE/source),'-o',str(a.output/binary)],check=True)
    sha=lambda path:hashlib.sha256(path.read_bytes()).hexdigest()
    data={'scope':'standalone matrix closure with real native typed rules',
          'sources':{str(HERE/name):sha(HERE/name) for name in [
              'build.py','compiler.py','kernel.metal','worker.swift','run.py',
              'dynamic.py','dynamic_client.py','dynamic_events.py','dynamic.metal','dynamic_worker.swift',
              'dynamic_fixture.egg','verify_dynamic.py','verify_dynamic_raytrace.py','replay_dynamic.py']},
          'worker_sha256':sha(a.output/'worker'),
          'dynamic_worker_sha256':sha(a.output/'dynamic-worker'),
          'oracle':json.loads((a.output/'oracle/build.json').read_text())}
    (a.output/'build.json').write_text(json.dumps(data,indent=2)+'\n')

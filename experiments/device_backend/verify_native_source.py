"""Compare a device endpoint with original native schedule execution.

Unlike replaying device-reported Run steps, this independently checks control
flow (Repeat, Saturate, until), not only matching at the chosen boundaries.
"""
import argparse,hashlib,json,subprocess
from pathlib import Path
from run import compare

def verify(source,build,device_run,output):
 output.mkdir(parents=True,exist_ok=False)
 subprocess.run([str((build/'frontend').resolve()),str(source.resolve()),'--native-full','--export-to',str((output/'native').resolve())],stdout=subprocess.DEVNULL,check=True)
 p=json.loads((device_run/'program.json').read_text());s=json.loads((device_run/'gpu-state.json').read_text());ref=json.loads((output/'native/native-state.json').read_text())
 helpers=set(p.get('assertion_tables',[]))
 rows=[r for r in s['rows'] if p['tables'][r[0]]['name'] not in helpers]
 result=compare(p,rows,s['parents'],ref,s['sets'])
 result['projected_control_tables']=sorted(helpers)
 result.update(original_native_schedule=True,source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),frontend_sha256=hashlib.sha256((build/'frontend').read_bytes()).hexdigest())
 (output/'verification.json').write_text(json.dumps(result,indent=2)+'\n');return result
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--build',type=Path,required=True);p.add_argument('--run',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();print(verify(a.source,a.build,a.run,a.output))

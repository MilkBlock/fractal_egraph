#!/usr/bin/env python3
"""Small reproducible ABI/arithmetic tests; GPU optional, no benchmark claim."""
import argparse
from pathlib import Path
import struct
import subprocess
import run

p=argparse.ArgumentParser()
p.add_argument('--backend',type=Path,default=run.ROOT/'out/cell-kernel-tools/backend')
p.add_argument('--gpu',action='store_true')
p.add_argument('--output',type=Path,default=run.ROOT/'out/cell-kernel-backend-tests')
a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
run.compile_plan(a.output)
rows=run.records(16)
# Exact native-i64-compatible arithmetic at the upper accepted GPU input range.
rows += [[99,1<<30,1<<30,1<<30,1<<30,1<<30,1<<30,15,1,1,1,1,1,1,1,1,1,1]]
# Strict size threshold: candidate 10 is rejected; old minimum9 permits it.
rows += [[100,4,4,1,1,1000,1000,15,1,1,1,1,1,1,1,1,1,1],
         [101,4,4,1,1,9,1000,15,1,1,1,1,1,1,1,1,1,1]]
input_path=a.output/'valid.bin'
input_path.write_bytes(b''.join(struct.pack('<18q',*r) for r in rows))
for mode in ['cpu']+(['gpu'] if a.gpu else []):
    out=a.output/f'{mode}.bin'
    cmd=[str(a.backend),mode,str(input_path),str(out)]
    if mode=='gpu':cmd+=['--metal-source',str(a.output/'kernel.metal')]
    result=subprocess.run(cmd,capture_output=True,text=True)
    assert result.returncode==0,result.stderr
    actual=list(struct.iter_unpack('<6q',out.read_bytes()))
    assert actual==[tuple(run.reference(r)) for r in rows]
for name,field,value in [('negative',1,-1),('too-large',1,(1<<30)+1),('alias',2,123)]:
    bad=rows[0].copy();bad[field]=value
    path=a.output/f'{name}.bin';path.write_bytes(struct.pack('<18q',*bad))
    result=subprocess.run([str(a.backend),'cpu',str(path),str(a.output/'bad-output.bin')],capture_output=True,text=True)
    assert result.returncode!=0,(name,result.stdout)
path=a.output/'truncated.bin';path.write_bytes(b'bad')
assert subprocess.run([str(a.backend),'cpu',str(path),str(a.output/'bad-output.bin')],capture_output=True).returncode!=0
empty=a.output/'empty.bin';empty.write_bytes(b'')
subprocess.run([str(a.backend),'cpu',str(empty),str(a.output/'empty-output.bin')],capture_output=True,check=True)
assert not (a.output/'empty-output.bin').read_bytes()
print(f'Passed {len(rows)} exact rows, 4 malformed input cases and empty CPU input; gpu={a.gpu}')

"""Small binary control/delta packets; GPU truth is read only for audit/export."""
import json
import hashlib
import os
from pathlib import Path
import struct
import subprocess
import numpy as np
from dynamic import pack

HERE=Path(__file__).resolve().parent

class Resident:
    def __init__(self,worker,library,out,mode='matrix',record_commands=False):
        out.mkdir(parents=True,exist_ok=True)
        path=out/'library.json';path.write_text(json.dumps(library))
        self.log=(out/'worker.log').open('w');self.up=0;self.down=0;self.epoch=0;self.commands=[];self.last_info={}
        self.record_commands=record_commands
        self.process=subprocess.Popen([str(worker.resolve()),str(path.resolve()),str(HERE/'kernel.metal'),str(HERE/'dynamic.metal')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log,env=dict(os.environ,MATRIX_CLOSURE_MODE=mode))
        if self.read(4)!=b'MCD2':raise RuntimeError('dynamic worker protocol')

    def read(self,n):
        value=bytearray()
        while len(value)<n:
            part=self.process.stdout.read(n-len(value))
            if not part:raise RuntimeError('dynamic worker ended; inspect worker.log')
            value.extend(part)
        return bytes(value)

    def send(self,words,allow_error=False):
        data=pack(words);self.process.stdin.write(data);self.process.stdin.flush();self.up+=len(data)
        n,p=struct.unpack('<2I',self.read(8));info=json.loads(self.read(n));payload=self.read(p);self.down+=8+n+p
        if self.record_commands:
            self.commands.append({'op':words[0],'words':words[:],'upload_bytes':len(data),'download_bytes':8+n+p,
                                  'payload_sha256':hashlib.sha256(payload).hexdigest(),'response':info})
        if not info['ok']:
            if allow_error:return info,payload
            raise RuntimeError(info['error'])
        self.epoch=info['epoch'];self.last_info=info;return info,payload

    def apply(self,patch):
        if patch['words'] is None:return dict(self.last_info,no_op_patch=True)
        return self.send(patch['words'])[0]
    def step(self,n=1):return self.send([2,self.epoch,n])[0]
    def advance(self,n=1):return self.send([5,self.epoch,n])[0]
    def saturate(self):return self.send([3,self.epoch])[0]
    def snapshot(self):
        info,payload=self.send([4,self.epoch]);n=info['fact_count']
        return info,np.frombuffer(payload,dtype='<u4').reshape(4,n)
    def close(self):
        if self.process.poll() is None:
            self.process.stdin.write(struct.pack('<I',0));self.process.stdin.flush();self.process.wait(timeout=30)
        for pipe in [self.process.stdin,self.process.stdout]:pipe.close()
        self.log.close()

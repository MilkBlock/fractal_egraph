"""Reuse the prepared deployment and alternate fresh native and Cell processes."""
import os,sys,json,time,statistics,subprocess,tempfile,hashlib
from pathlib import Path
cap,compiler,bake,donor,linker,native,cell,scratch,output=map(Path,sys.argv[1:])
base={k:v for k,v in os.environ.items() if not k.startswith(('CELL_CPU_','EGG_LAYOUT_'))};base.update(RAYON_NUM_THREADS='1',CELL_CPU_STAGE_SPINE='1',CELL_CPU_STAGE_SPINE_RECEIPTS='1',CELL_CPU_SHARED_NODE_SCHEDULE='1',CELL_CPU_JOIN_INTERSECTION='1',CELL_CPU_SIEVE='1',CELL_CPU_FREE_JOIN='planned',CELL_CPU_KNOWN_RESULTS='static',CELL_CPU_RANGE_GUARDS='1',CELL_CPU_FD_KEYS='0',CELL_CPU_SCALAR_MERGE='0',CELL_CPU_SLOT_TRANSFER='off')
def run(xs):return subprocess.check_output(list(map(str,xs)),env=base,stderr=subprocess.PIPE,text=True)
records=[]
for dirname in ('032-sum-bits','064-fenwick_tree','095-raytrace'):
 src=cap/dirname/'pass-3-flat.egg'
 with tempfile.TemporaryDirectory(dir=scratch) as td:
  prep=time.perf_counter();w=Path(td);run([compiler,'compile','--source',src,'--bake-source',bake,'--emit-free-join-entry','--output',w/'compiled']);run([linker,donor,w/'compiled/program.json',w/'ready.json'])
  prep=time.perf_counter()-prep
  for k in range(5):
   record={'directory':dirname,'block':k,'prepare_s':prep}
   for mode in (('native','cell') if k%2 else ('cell','native')):
    if mode=='native':
     tick=time.perf_counter();record['native_ms']=float(run([native,src]).strip().splitlines()[-1].split(' ms')[0].split()[-1]);record['native_cli_ms']=1000*(time.perf_counter()-tick)
    else:
     with tempfile.TemporaryDirectory(dir=w) as sd:
      out=Path(sd)/'out';tick=time.perf_counter();run([cell,'--program',w/'ready.json','--output',out]);record['cell_cli_ms']=1000*(time.perf_counter()-tick);record['cell_ms']=1000*json.load(open(out/'report.json'))['inference_seconds']
      with open(out/'state.json','rb') as f:record['state_sha256']=hashlib.file_digest(f,'sha256').hexdigest()
   records.append(record);output.write_text(json.dumps(records,indent=2))

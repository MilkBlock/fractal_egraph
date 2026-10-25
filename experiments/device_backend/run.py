"""CPU controls only. GPU owns all evolving tables/equalities/effects.

The exact row/anchor comparison follows the existing live_table_bench.rs
comparison contract; GPU state is never reimported and repaired by native rebuild.
"""
import argparse,json,struct,subprocess,time,hashlib
from pathlib import Path
from compiler import compile_program

def read(pipe,n):
 b=b''
 while len(b)<n:
  c=pipe.read(n-len(b))
  if not c:raise RuntimeError('worker terminated; inspect stderr log')
  b+=c
 return b

def compare(p,rows,parents,checkpoint,arena=None):
 def find(x):
  assert 0<=x<len(parents),'invalid device identity'
  seen=set()
  while parents[x]!=x:
   if x in seen or parents[x]>=len(parents):raise AssertionError('invalid device union forest')
   seen.add(x);x=parents[x]
  return x
 forward={};inverse={}
 def pair(a,b):
  if forward.get(a,b)!=b or inverse.get(b,a)!=a:raise AssertionError('different equality partition')
  forward[a]=b;inverse[b]=a
 anchors={(a['sort'],a['old']):a['now'] for a in checkpoint['anchors']}
 for i,key in enumerate(p['anchors']):pair((key[0],find(i+1)),(key[0],anchors[tuple(key)]))
 def native_value(x):
  if 'elements' in x:return (x['sort'],frozenset(native_value(e) for e in x['elements']))
  return (x['sort'],x['eq'] if 'eq' in x else x['value'])
 native={}
 for r in checkpoint['rows']:
  vals=tuple(native_value(x) for x in r['values'])
  k=(r['table'],vals[:-1]);assert k not in native,'duplicate native key'
  native[k]=(vals[-1],r['subsumed'])
 pending=[];keys=set()
 for r in rows:
  assert len(r)==p['stride'] and 0<=r[0]<len(p['tables']) and r[1] in (0,1),'invalid device row header'
  t=p['tables'][r[0]];vals=[]
  for s,x in zip(t['schema'],r[4:]):
   if s['kind']=='eq':assert find(x)==x,'GPU left a noncanonical row'
   if s['kind']=='String':assert 0<=x<len(p['strings']),'invalid device string handle'
   if s['kind']=='set':
    assert arena is not None and 2<=x<arena[0],'invalid set handle'
    es=arena[x+4:x+4+arena[x+1]]
    assert list(es)==sorted(set(es)) and all(find(e)==e for e in es),'noncanonical set'
    vals.append((s['name'],frozenset((s['element']['name'],e) for e in es)))
   else:vals.append((s['name'],p['strings'][x] if s['kind']=='String' else x))
  key=(t['name'],tuple(vals[:-1]));assert key not in keys,'GPU left duplicate canonical keys';keys.add(key)
  pending.append((t,vals,bool(r[1])))
 assert len(native)==len(pending),f'row count differs {len(native)} vs {len(pending)}'
 visited=0
 while pending:
  rest=[]
  for t,vs,hidden in pending:
   mapped=[]
   for s,v in zip(t['schema'][:-1],vs[:-1]):
    if s['kind']=='set':
     if any(e not in forward for e in v[1]):break
     mapped.append((v[0],frozenset(forward[e] for e in v[1])));continue
    if s['kind']=='eq' and v not in forward:break
    mapped.append(forward[v] if s['kind']=='eq' else v)
   else:
    key=(t['name'],tuple(mapped));assert key in native,f'missing native row {key}'
    value,vis=native[key];assert hidden==vis,f'visibility mismatch {key}'
    if t['schema'][-1]['kind']=='eq':pair(vs[-1],value)
    elif t['schema'][-1]['kind']=='set':
     if not all(e in forward for e in vs[-1][1]):rest.append((t,vs,hidden));continue
     assert (vs[-1][0],frozenset(forward[e] for e in vs[-1][1]))==value,f'set value mismatch {key}'
    else:assert vs[-1]==value,f'value mismatch {key}'
    visited+=1;continue
   rest.append((t,vs,hidden))
  assert len(rest)<len(pending),'unmapped new cycle: comparison inconclusive'
  pending=rest
 return {'exact_rows_values_equivalence_visibility':True,'rows':visited,'eclasses':len(forward)}

def run(source,build,out,check_every=1,capacity=4096,match_capacity=16384,check_at_end=False):
 out.mkdir(parents=True,exist_ok=True)
 if (out/'report.json').exists():raise ValueError('output already contains a successful run; use a new directory')
 with (out/'native.log').open('w') as nl,(out/'gpu.log').open('w') as gl:
  native=subprocess.Popen([str((build/'frontend').resolve()),str(source.resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=nl,text=True)
  gpu=None;steps=[];phase="compile"
  try:
   line=native.stdout.readline()
   if not line:raise RuntimeError('native compilation failed; see native.log')
   original=json.loads(line);(out/'core.json').write_text(json.dumps(original,indent=2))
   p=compile_program(original,capacity,match_capacity);del original
   (out/'program.json').write_text(json.dumps(p));(out/'kernel.metal').write_text(p['shader'])
   gpu=subprocess.Popen([str((build/'worker').resolve()),str((out/'program.json').resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=gl)
   assert read(gpu.stdout,4)==b'DVB1'
   initial_bytes=(len(p.pop('initial_rows'))+len(p.pop('initial_sets')))*8
   if not check_every and not check_at_end:
    native.stdin.write('{"op":"quit"}\n');native.stdin.flush();native.wait(timeout=30)
   phase="run"
   steps=[];checks=[];up=0;down=0;snapshot_bytes=0;oracle_runs=0
   def oracle(q):
    native.stdin.write(json.dumps(q)+'\n');native.stdin.flush();s=native.stdout.readline()
    if not s:raise RuntimeError('native oracle failed')
    return json.loads(s)
   def command(op,arg=0):
    nonlocal up
    gpu.stdin.write(struct.pack('<II',op,arg));gpu.stdin.flush();up+=8
   def checkpoint():
    nonlocal snapshot_bytes
    command(2);meta=struct.unpack('<9I',read(gpu.stdout,36));n=meta[0]*p['stride']
    raw=struct.unpack('<'+'q'*n,read(gpu.stdout,n*8));rows=[raw[i:i+p['stride']] for i in range(0,n,p['stride'])]
    parents=struct.unpack('<'+'I'*meta[1],read(gpu.stdout,meta[1]*4));facts=struct.unpack('<'+'I'*meta[8],read(gpu.stdout,meta[8]*4))
    arena=struct.unpack('<'+'q'*p['set_words'],read(gpu.stdout,p['set_words']*8))
    snapshot_bytes+=36+n*8+len(parents)*4+len(facts)*4+len(arena)*8
    (out/'gpu-state.json').write_text(json.dumps({'meta':meta,'rows':rows,'parents':parents,'fact_parents':facts,'sets':arena}))
    reference=oracle({'op':'snapshot'});(out/'native-state.json').write_text(json.dumps(reference))
    result=compare(p,rows,parents,reference,arena);result['step']=len(steps);checks.append(result)
   def advance(s):
    nonlocal down,oracle_runs
    if 'run' in s:
     setid=p['sets'].index(s['run']);command(1,setid);m=struct.unpack('<9I',read(gpu.stdout,36));down+=36
     if s.get('assert') and not m[5] and not m[4]:raise AssertionError('GPU source check failed')
     if m[4]:raise RuntimeError(f'GPU incomplete, error={m[4]} (1 storage capacity, 2 match capacity, 3 missing lookup, 4 conflicting function, 5 overflow, 6 epoch exhaustion, 7 ambiguous ordered merge)')
     steps.append({'set':s['run'],'rows':m[0],'matched':m[5],'updated':bool(m[3]),'epoch':m[2],'unions':m[7]})
     if check_every and not check_at_end:
      ref=oracle({'op':'run','set':s['run']});oracle_runs+=1
      if bool(m[3])!=ref['updated']:
       checkpoint();raise AssertionError('CPU/GPU progress mismatch')
      if len(steps)%check_every==0:checkpoint()
     return bool(m[3])
    if 'sequence' in s:
     changed=False
     for x in s['sequence']:changed=advance(x) or changed
     return changed
    if 'repeat' in s:
     changed=False
     for _ in range(s['repeat']):
      updated=advance(s['body']);changed=updated or changed
      if not updated:break
     return changed
    if 'saturate' in s:
     any_change=False
     while advance(s['saturate']):any_change=True
     return any_change
    raise ValueError('schedule')
   started=time.perf_counter()
   for s in p['schedule']:advance(s)
   if check_at_end:
    phase='post-run-oracle'
    for step in steps:
     ref=oracle({'op':'run','set':step['set']});oracle_runs+=1
     if ref['updated']!=step['updated']:raise AssertionError('post-run CPU/GPU progress mismatch')
    checkpoint()
   if check_every and not check_at_end and (not checks or checks[-1]['step']!=len(steps)):checkpoint()
   elapsed=time.perf_counter()-started
   command(0);gpu.wait(timeout=30)
   if native.poll() is None:
    native.stdin.write('{"op":"quit"}\n');native.stdin.flush();native.wait(timeout=30)
   result={'scope':'native-compiled .egg, experimental GPU-owned execution backend; serial device reference kernel, not a throughput benchmark','steps':steps,'checks':checks,'checked':bool(check_every or check_at_end),'check_mode':'final' if check_at_end else ('periodic' if check_every else 'off'),'native_oracle_runs':oracle_runs,'run_control_upload_bytes':8*len(steps),'run_status_download_bytes':down,'checkpoint_bytes':snapshot_bytes,'intermediate_binding_download_bytes':0,'cpu_rhs_calls':0,'cpu_rebuild_calls_outside_oracle':0,'wall_seconds_including_debug_checks':elapsed,'initial_row_upload_bytes':initial_bytes,'native_retired_before_gpu_runs':not bool(check_every or check_at_end),'oracle_starts_after_gpu':check_at_end,'control_upload_bytes':up,'assertion_tables':p.get('assertion_tables',[]),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'build':json.loads((build/'build.json').read_text()),'program_sha256':hashlib.sha256((out/'program.json').read_bytes()).hexdigest()}
   (out/'report.json').write_text(json.dumps(result,indent=2)+'\n');return result
  except Exception as e:
   (out/'failure.json').write_text(json.dumps({'phase':phase,'error':str(e),'completed_steps':steps},indent=2));raise
  finally:
   for proc in [gpu,native]:
    if proc is not None:
     if proc.poll() is None:proc.terminate();proc.wait(timeout=15)
     if proc.stdin:proc.stdin.close()
     if proc.stdout:proc.stdout.close()

def main():
 a=argparse.ArgumentParser();a.add_argument('--source',type=Path,required=True);a.add_argument('--build',type=Path,required=True);a.add_argument('--output',type=Path,required=True);a.add_argument('--check-every',type=int,default=1,help='0 disables CPU execution; positive N verifies every N Run boundaries');a.add_argument('--check-at-end',action='store_true',help='replay native only after all GPU Run commands finish');a.add_argument('--capacity',type=int,default=4096);a.add_argument('--match-capacity',type=int,default=16384);x=a.parse_args()
 if x.check_every<0:a.error('negative checkpoint interval')
 print(json.dumps(run(x.source,x.build,x.output,x.check_every,x.capacity,x.match_capacity,x.check_at_end),indent=2))
if __name__=='__main__':main()

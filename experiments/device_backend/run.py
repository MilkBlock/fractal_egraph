"""CPU controls only. GPU owns all evolving tables/equalities/effects.

The exact row/anchor comparison follows the existing live_table_bench.rs
comparison contract; GPU state is never reimported and repaired by native rebuild.
"""
import argparse,json,struct,subprocess,time,hashlib,os,re
from pathlib import Path
from compiler import compile_program

def read(pipe,n):
 b=b''
 while len(b)<n:
  c=pipe.read(n-len(b))
  if not c:raise RuntimeError('worker terminated; inspect stderr log')
  b+=c
 return b

def atomic_json(path,value):
 temporary=path.with_name(path.name+'.pending')
 temporary.write_text(json.dumps(value));temporary.replace(path)

def preserve_verified(source,target):
 temporary=target.with_name(target.name+'.pending')
 temporary.unlink(missing_ok=True);os.link(source,temporary);temporary.replace(target)

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

def load_resume(folder,original,p,require_native=True):
 """Only restore a complete, native-verified Run boundary of the same core IR."""
 if json.loads((folder/'core.json').read_text())!=original:raise ValueError('resume core program differs')
 certified=require_native and (folder/'verified-checkpoint.json').exists()
 state=json.loads((folder/('verified-gpu-state.json' if certified else 'gpu-state.json')).read_text())
 if len(state['meta'])!=9 or len(state['parents'])!=state['meta'][1] or len(state['fact_parents'])!=state['meta'][8]:raise ValueError('invalid checkpoint lengths')
 old=json.loads((folder/'program.json').read_text())
 if p['tables']!=old['tables'] or p['strings']!=old['strings'] or [list(a) for a in p['anchors']]!=old['anchors']:raise ValueError('resume identities differ')
 if require_native:
  native=json.loads((folder/('verified-native-state.json' if certified else 'native-state.json')).read_text());compare(p,state['rows'],state['parents'],native,state['sets'])
 else:
  from export_snapshot import gpu_rows
  for field in ['parents','fact_parents']:
   if any(parent<0 or parent>i for i,parent in enumerate(state[field])):raise ValueError('invalid minimum-root forest')
  def freeze(v):
   if 'elements' in v:return (v['sort'],frozenset(freeze(x) for x in v['elements']))
   return (v['sort'],v.get('eq',v.get('value')))
  keys=set()
  for row in gpu_rows(p,state):
   key=(row['table'],tuple(freeze(x) for x in row['values'][:-1]))
   if key in keys:raise ValueError('duplicate canonical GPU key')
   keys.add(key)
 saved=json.loads((folder/('verified-checkpoint.json' if certified else 'checkpoint-meta.json' if (folder/'checkpoint-meta.json').exists() else 'report.json' if (folder/'report.json').exists() else 'failure.json')).read_text())
 steps=saved.get('steps',saved.get('completed_steps'));n=state['meta'][6]
 if not steps or n<1 or n>len(steps) or state['meta'][4]:raise ValueError('resume is not a completed boundary')
 steps=steps[:n];last=steps[-1]
 if (last['rows'],last['epoch'],last['unions'])!=(state['meta'][0],state['meta'][2],state['meta'][7]):raise ValueError('resume boundary metadata differs')
 if state['meta'][0]>p['capacity'] or max(state['meta'][1],state['meta'][8])>p['id_capacity'] or len(state['sets'])>p['set_words']:raise ValueError('resume exceeds capacity')
 wm=[0]*p['rules']
 for s in steps:
  for r in p['set_rule_ids'][p['sets'].index(s['set'])]:wm[r]=s['epoch']-1
 p.update(initial_rows=[v for row in state['rows'] for v in row],initial_sets=state['sets'],meta=state['meta'],initial_parents=state['parents'],initial_fact_parents=state['fact_parents'],initial_watermarks=wm)
 return steps

def run(source,build,out,check_every=1,capacity=4096,match_capacity=16384,check_at_end=False,max_runs=None,defer_unions=True,incremental_queries=True,resume=None,delta_queries=True,verify_against=None,save_final_state=False,parallel_rebuild=True,parallel_prepare=True,parallel_match=True,require_native_resume=True,snapshot_every=0,indexed_subsume=True,parallel_merge_index=True):
 setup_started=time.perf_counter()
 if any(d is not None and d.resolve()==out.resolve() for d in [resume,verify_against]):raise ValueError('output must differ from resume/reference directory')
 out.mkdir(parents=True,exist_ok=True)
 if verify_against is not None:check_every=0;check_at_end=False
 if (out/'report.json').exists():raise ValueError('output already contains a successful run; use a new directory')
 with (out/'native.log').open('w') as nl,(out/'gpu.log').open('w') as gl:
  native=subprocess.Popen([str((build/'frontend').resolve()),str(source.resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=nl,text=True)
  gpu=None;steps=[];phase="compile";timings={"device_control_wait":0.0,"device_gpu":0.0,"prepare":0.0,"match":0.0,"commit":0.0,"native_oracle":0.0,"checkpoint_excluding_oracle":0.0,"comparison":0.0,"state_export_io":0.0};device_times=[]
  try:
   line=native.stdout.readline()
   if not line:raise RuntimeError('native compilation failed; see native.log')
   original=json.loads(line);(out/'core.json').write_text(json.dumps(original,indent=2))
   control=None
   if verify_against is not None:
    if json.loads((verify_against/'core.json').read_text())!=original:raise ValueError('stored reference core differs')
    control=json.loads((verify_against/'report.json').read_text())
    if not control['checks'] or control['checks'][-1]['step']!=len(control['steps']):raise ValueError('stored reference lacks a final native checkpoint')
   p=compile_program(original,capacity,match_capacity,defer_unions,incremental_queries,delta_queries,parallel_rebuild,parallel_prepare,parallel_match,indexed_subsume,parallel_merge_index)
   prefix=load_resume(resume,original,p,require_native_resume) if resume else []
   if max_runs is not None and max_runs<=len(prefix):raise ValueError('max-runs must exceed resumed prefix')
   del original
   (out/'program.json').write_text(json.dumps(p));(out/'kernel.metal').write_text(p['shader'])
   gpu=subprocess.Popen([str((build/'worker').resolve()),str((out/'program.json').resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=gl)
   assert read(gpu.stdout,4)==b'DVB4'
   initial_bytes=(len(p.pop('initial_rows'))+len(p.pop('initial_sets'))+len(p.pop('action_code')))*8
   initial_bytes+=sum(len(p.pop(k,[]))*4 for k in ['initial_parents','initial_fact_parents','initial_watermarks'])
   if not check_every and not check_at_end:
    native.stdin.write('{"op":"quit"}\n');native.stdin.flush();native.wait(timeout=30)
   setup_seconds=time.perf_counter()-setup_started
   phase="run"
   steps=[];checks=[];up=0;down=0;snapshot_bytes=0;oracle_runs=0
   def oracle(q):
    t=time.perf_counter()
    native.stdin.write(json.dumps(q)+'\n');native.stdin.flush();s=native.stdout.readline()
    if not s:raise RuntimeError('native oracle failed')
    result=json.loads(s);timings["native_oracle"]+=time.perf_counter()-t;return result
   def command(op,arg=0):
    nonlocal up
    gpu.stdin.write(struct.pack('<II',op,arg));gpu.stdin.flush();up+=8
   def read_gpu_state():
    nonlocal snapshot_bytes
    read_started=time.perf_counter()
    command(2);meta=struct.unpack('<9I',read(gpu.stdout,36));n=meta[0]*p['stride']
    raw=struct.unpack('<'+'q'*n,read(gpu.stdout,n*8));rows=[raw[i:i+p['stride']] for i in range(0,n,p['stride'])]
    parents=struct.unpack('<'+'I'*meta[1],read(gpu.stdout,meta[1]*4));facts=struct.unpack('<'+'I'*meta[8],read(gpu.stdout,meta[8]*4))
    live_sets=struct.unpack('<I',read(gpu.stdout,4))[0];assert 2<=live_sets<=p['set_words']
    arena=struct.unpack('<'+'q'*live_sets,read(gpu.stdout,live_sets*8))
    snapshot_bytes+=40+n*8+len(parents)*4+len(facts)*4+len(arena)*8
    atomic_json(out/'gpu-state.json',{'meta':meta,'rows':rows,'parents':parents,'fact_parents':facts,'sets':arena})
    atomic_json(out/'checkpoint-meta.json',{'steps':steps,'native_verified':False})
    timings["state_export_io"]+=time.perf_counter()-read_started
    return meta,rows,parents,arena
   def checkpoint(certify=True):
    t=time.perf_counter();oracle_before=timings["native_oracle"]
    meta,rows,parents,arena=read_gpu_state()
    reference=json.loads((verify_against/'native-state.json').read_text()) if verify_against else oracle({'op':'snapshot'});atomic_json(out/'native-state.json',reference)
    ct=time.perf_counter();result=compare(p,rows,parents,reference,arena);timings['comparison']+=time.perf_counter()-ct;result['step']=len(steps);checks.append(result)
    if certify:
     preserve_verified(out/'gpu-state.json',out/'verified-gpu-state.json');preserve_verified(out/'native-state.json',out/'verified-native-state.json')
     atomic_json(out/'verified-checkpoint.json',{'steps':steps,'check':result})
    timings['checkpoint_excluding_oracle']+=time.perf_counter()-t-(timings['native_oracle']-oracle_before)
   class PrefixComplete(Exception):pass
   def advance(s):
    nonlocal down,oracle_runs
    if 'run' in s:
     if len(steps)<len(prefix):
      expected=prefix[len(steps)]
      if expected['set']!=s['run']:raise ValueError('resume schedule differs')
      steps.append(expected)
      if check_every and not check_at_end:
       ref=oracle({'op':'run','set':s['run']});oracle_runs+=1
       if ref['updated']!=expected['updated']:raise AssertionError('resume native prefix differs')
       if len(steps)==len(prefix):checkpoint()
      return expected['updated']
     if max_runs is not None and len(steps)>=max_runs:raise PrefixComplete()
     setid=p['sets'].index(s['run']);
     if os.environ.get('EGG_DEVICE_PHASE_LOG')=='1':atomic_json(out/'pending-command.json',{'step':len(steps)+1,'set':s['run']})
     t=time.perf_counter();command(1,setid);m=struct.unpack('<9I',read(gpu.stdout,36));phase_seconds=struct.unpack('<3d',read(gpu.stdout,24));gpu_seconds=sum(phase_seconds);index_rebuilds=struct.unpack('<q',read(gpu.stdout,8))[0];down+=68
     for name,value in zip(['prepare','match','commit'],phase_seconds):timings[name]+=value
     timings['device_control_wait']+=time.perf_counter()-t;timings['device_gpu']+=gpu_seconds;device_times.append({'set':s['run'],'gpu_seconds':gpu_seconds,'prepare_seconds':phase_seconds[0],'match_seconds':phase_seconds[1],'commit_seconds':phase_seconds[2],'index_rebuilds':index_rebuilds})
     if s.get('assert') and not m[5] and not m[4]:raise AssertionError('GPU source check failed')
     if m[4]:raise RuntimeError(f'GPU incomplete, error={m[4]} (1 storage capacity, 2 match capacity, 3 missing lookup, 4 conflicting function, 5 overflow, 6 epoch exhaustion, 7 ambiguous ordered merge)')
     steps.append({'set':s['run'],'rows':m[0],'matched':m[5],'updated':bool(m[3]),'epoch':m[2],'unions':m[7]})
     if len(steps)%32==0:(out/'progress.json').write_text(json.dumps({'step':len(steps),'last':steps[-1],'timings_seconds':timings}))
     if check_every and not check_at_end:
      ref=oracle({'op':'run','set':s['run']});oracle_runs+=1
      if bool(m[3])!=ref['updated']:
       checkpoint(certify=False);raise AssertionError('CPU/GPU progress mismatch')
      if len(steps)%check_every==0:checkpoint()
     if snapshot_every and len(steps)%snapshot_every==0 and (not checks or checks[-1]['step']!=len(steps)):read_gpu_state()
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
   complete=True
   try:
    for s in p['schedule']:advance(s)
   except PrefixComplete:complete=False
   schedule_seconds=time.perf_counter()-started
   if check_at_end:
    phase='post-run-oracle'
    for step in steps:
     ref=oracle({'op':'run','set':step['set']});oracle_runs+=1
     if ref['updated']!=step['updated']:raise AssertionError('post-run CPU/GPU progress mismatch')
    checkpoint()
   if check_every and not check_at_end and (not checks or checks[-1]['step']!=len(steps)):checkpoint()
   if control is not None:
    key=lambda ss:[(s['set'],s['updated']) for s in ss]
    if key(steps)!=key(control['steps']):raise AssertionError('stored reference schedule differs')
    checkpoint()
   if save_final_state and (not checks or checks[-1]['step']!=len(steps)):read_gpu_state()
   elapsed=time.perf_counter()-started
   command(0);gpu.wait(timeout=30)
   if native.poll() is None:
    native.stdin.write('{"op":"quit"}\n');native.stdin.flush();native.wait(timeout=30)
   clean_timing=(not check_every or check_at_end) and not snapshot_every and os.environ.get('EGG_DEVICE_PHASE_LOG')!='1' and os.environ.get('MTL_DEBUG_LAYER')!='1'
   allocated=re.search(r'device allocated_bytes=(\d+)',(out/'gpu.log').read_text())
   result={'parallel_merge_index':parallel_merge_index,'setup_seconds':setup_seconds,'schedule_seconds':schedule_seconds,'inference_seconds':schedule_seconds if clean_timing else None,'clean_timing':clean_timing,'gpu_allocated_bytes_at_start':int(allocated.group(1)) if allocated else None,'indexed_subsume':indexed_subsume,'resume_validation':'native' if require_native_resume else 'GPU canonical state only','snapshot_every':snapshot_every,'parallel_match':parallel_match,'parallel_prepare':parallel_prepare,'parallel_rebuild':parallel_rebuild,'state_export_requested':save_final_state,'phase_logging':os.environ.get('EGG_DEVICE_PHASE_LOG')=='1','verify_against':str(verify_against.resolve()) if verify_against else None,'resume_directory':str(resume.resolve()) if resume else None,'resumed_prefix_runs':len(prefix),'incremental_queries':incremental_queries,'delta_queries':delta_queries,'deferred_rulesets':p['deferred_rulesets'],'timings_seconds':timings,'device_command_times':device_times,'complete_schedule':complete,'requested_max_runs':max_runs,'scope':'native-compiled .egg, experimental GPU-owned execution backend; consult clean_timing, completeness and validation scope before using timings','steps':steps,'checks':checks,'checked':bool(check_every or check_at_end or verify_against),'check_mode':'stored' if verify_against else 'final' if check_at_end else ('periodic' if check_every else 'off'),'native_oracle_runs':oracle_runs,'run_control_upload_bytes':8*len(device_times),'run_status_download_bytes':down,'checkpoint_bytes':snapshot_bytes,'intermediate_binding_download_bytes':0,'cpu_rhs_calls':0,'cpu_rebuild_calls_outside_oracle':0,'wall_seconds_including_debug_checks':elapsed,'initial_row_upload_bytes':initial_bytes,'native_retired_before_gpu_runs':not bool(check_every or check_at_end),'oracle_starts_after_gpu':check_at_end,'control_upload_bytes':up,'assertion_tables':p.get('assertion_tables',[]),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'build':json.loads((build/'build.json').read_text()),'program_sha256':hashlib.sha256((out/'program.json').read_bytes()).hexdigest()}
   (out/'report.json').write_text(json.dumps(result,indent=2)+'\n');return result
  except Exception as e:
   (out/'failure.json').write_text(json.dumps({'phase':phase,'error':str(e),'completed_steps':steps,'timings_seconds':timings,'device_command_times':device_times},indent=2));raise
  finally:
   for proc in [gpu,native]:
    if proc is not None:
     if proc.poll() is None:proc.terminate();proc.wait(timeout=15)
     if proc.stdin:proc.stdin.close()
     if proc.stdout:proc.stdout.close()

def main():
 a=argparse.ArgumentParser();a.add_argument('--source',type=Path,required=True);a.add_argument('--build',type=Path,required=True);a.add_argument('--output',type=Path,required=True);a.add_argument('--check-every',type=int,default=1,help='0 disables CPU execution; positive N verifies every N Run boundaries');a.add_argument('--check-at-end',action='store_true',help='replay native only after all GPU Run commands finish');a.add_argument('--capacity',type=int,default=4096);a.add_argument('--match-capacity',type=int,default=16384);a.add_argument('--serial-merge-index',action='store_true',help='diagnostic index rebuild inside merge callbacks');a.add_argument('--scan-subsume',action='store_true',help='diagnostic full-table subsumption');a.add_argument('--resume-gpu',type=Path,help='restore canonical GPU state without claiming native equality');a.add_argument('--snapshot-every',type=int,default=0,help='diagnostic GPU-only checkpoints');a.add_argument('--serial-match',action='store_true',help='diagnostic serial matcher');a.add_argument('--serial-prepare',action='store_true',help='diagnostic ordered-chain preparation');a.add_argument('--serial-rebuild',action='store_true',help='diagnostic serial rebuild reference');a.add_argument('--save-state',action='store_true',help='export final GPU state without requiring native execution');a.add_argument('--verify-against',type=Path,help='reuse an exact native final checkpoint; no native Run replay');a.add_argument('--no-delta-queries',action='store_true',help='diagnostic: filter changed bindings only after joining');a.add_argument('--resume',type=Path,help='resume from a native-verified checkpoint; not an end-to-end timing run');a.add_argument('--rebuild-query-index',action='store_true',help='diagnostic: rebuild snapshot indexes on every Run');a.add_argument('--eager-unions',action='store_true',help='diagnostic legacy mode; may differ semantically from native');a.add_argument('--max-runs',type=int,help='diagnostic prefix only, reported as incomplete');x=a.parse_args()
 if x.resume and x.resume_gpu:a.error('choose one resume mode')
 if x.snapshot_every<0:a.error('snapshot interval must be nonnegative')
 if x.check_every<0:a.error('negative checkpoint interval')
 if x.max_runs is not None and x.max_runs<1:a.error('max-runs must be positive')
 print(json.dumps(run(x.source,x.build,x.output,x.check_every,x.capacity,x.match_capacity,x.check_at_end,x.max_runs,not x.eager_unions,not x.rebuild_query_index,x.resume or x.resume_gpu,not x.no_delta_queries,x.verify_against,x.save_state,not x.serial_rebuild,not x.serial_prepare,not x.serial_match,x.resume_gpu is None,x.snapshot_every,not x.scan_subsume,not x.serial_merge_index),indent=2))
if __name__=='__main__':main()

"""CPU controls only. GPU owns all evolving tables/equalities/effects.

The exact row/anchor comparison follows the existing live_table_bench.rs
comparison contract; GPU state is never reimported and repaired by native rebuild.
"""
import argparse,json,struct,subprocess,time,hashlib,os,re
from pathlib import Path
from compiler import compile_program
from joint_polyspine import normalize_schedule

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
  if s.get('kind')=='joint_polyspine':
   for name,epoch in s['ruleset_watermarks'].items():
    for r in p['set_rule_ids'][p['sets'].index(name)]:wm[r]=epoch
   continue
  for r in p['set_rule_ids'][p['sets'].index(s['set'])]:wm[r]=s['epoch'] if s.get('kind')=='comparison_saturate' else s['epoch']-1
 p.update(initial_rows=[v for row in state['rows'] for v in row],initial_sets=state['sets'],meta=state['meta'],initial_parents=state['parents'],initial_fact_parents=state['fact_parents'],initial_watermarks=wm)
 return steps

def run(source,build,out,check_every=1,capacity=4096,match_capacity=16384,check_at_end=False,max_runs=None,defer_unions=True,incremental_queries=True,resume=None,delta_queries=True,verify_against=None,save_final_state=False,parallel_rebuild=True,parallel_prepare=True,parallel_match=True,require_native_resume=True,snapshot_every=0,indexed_subsume=True,parallel_merge_index=True,all_index_columns=False,radix_digit_bits=4,seed_dispatch=True,noop_preflight=True,direct_actions=True,query_patch=True,effect_dedup=False,early_seed_guards=False,lhs_context_reuse=True,rebuild_batch=1,parallel_rhs=False,split_matches=False,context_model=None,context_training=False,merge_target_index=False,batch_rhs=False,matrix_saturate=False,joint_polyspine=False,polyspine_model=None,polyspine_training=False,polyspine_full_refresh=False,polyspine_refresh_audit=False,stage_contexts=False,bake_stage_cell=None,stage_cell=None,stage_context_audit=False,stage_context_export=False,block_merge=None,cell_program=None,scheduled_matrix=False,schedule_library=None,marker_program=None,compiled_program=None):
 scheduled_matrix=scheduled_matrix or schedule_library is not None
 setup_started=time.perf_counter()
 stage_contexts=stage_contexts or bake_stage_cell is not None or stage_cell is not None
 block_merge=(stage_contexts or cell_program is not None) if block_merge is None else block_merge
 if cell_program is not None and not block_merge:raise ValueError("executable Cell recipes require staged block merge")
 joint_polyspine=joint_polyspine or stage_contexts or block_merge or cell_program is not None
 if stage_contexts and (polyspine_model is not None or polyspine_training):raise ValueError('stage Cell currently uses exact contexts without ridge/training')
 if bake_stage_cell is not None and bake_stage_cell.exists():raise ValueError('baked Cell already exists')
 stage_context_audit=stage_context_audit or bake_stage_cell is not None
 if (stage_context_audit or stage_context_export) and not stage_contexts:raise ValueError('stage audit/export requires --stage-contexts or a baked Cell')
 if (polyspine_model is not None or polyspine_training or polyspine_full_refresh or polyspine_refresh_audit) and not joint_polyspine:raise ValueError('Polyspine model/training/refresh controls require --joint-polyspine')
 if polyspine_training and (out/'polyspine-training').exists():raise ValueError('training output exists; use a fresh run directory')
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
   executable=None
   if cell_program is not None:
    from tools.cell_spine.executable import load as load_cell_program
    executable=load_cell_program(cell_program,original)
   marker_ir=json.loads(marker_program.read_text()) if marker_program is not None else None
   if marker_ir is not None and marker_ir["source_sha256"]!=hashlib.sha256(source.read_bytes()).hexdigest():raise ValueError("marker source fingerprint mismatch")
   if compiled_program is not None:
    folder=compiled_program.parent
    manifest=json.loads((folder/'manifest.json').read_text())
    if manifest['schema']!='eggfuse-device-artifact/v1':raise ValueError('device manifest schema')
    if manifest.get('backend')!='gpu':raise ValueError('compiled artifact requires its declared backend')
    for filename,expected in manifest['files'].items():
     if Path(filename).name!=filename:raise ValueError('invalid artifact filename')
     if hashlib.sha256((folder/filename).read_bytes()).hexdigest()!=expected:raise ValueError('compiled artifact checksum mismatch: '+filename)
    if json.loads((folder/'core.json').read_text())!=original:raise ValueError('compiled artifact input/type/control differs')
    if manifest['source_sha256']!=hashlib.sha256(source.read_bytes()).hexdigest():raise ValueError('compiled source differs')
    if marker_ir is None or manifest['marker_sha256']!=marker_ir['sha256']:raise ValueError('compiled marker contract differs')
    from tools.eggfuse.marker_ir import validate as validate_marker_ir
    validate_marker_ir(marker_ir,original)
    p=json.loads(compiled_program.read_text())
    if p['shader']!=(folder/'kernel.metal').read_text() or p['tables']!=original['tables']:raise ValueError('compiled payload and emitted kernel/schema differ')
    if p['marker_program']['sha256']!=marker_ir['sha256']:raise ValueError('compiled marker descriptor differs')
    from tools.eggfuse.multistage import validate_runtime_options
    validate_runtime_options(manifest,p,dict(stage_contexts=stage_contexts,joint_polyspine=joint_polyspine,block_merge=block_merge,matrix_saturate=matrix_saturate),executable)
   else:
    p=compile_program(original,capacity,match_capacity,defer_unions,incremental_queries,delta_queries,parallel_rebuild,parallel_prepare,parallel_match,indexed_subsume,parallel_merge_index,all_index_columns,radix_digit_bits,seed_dispatch,noop_preflight,direct_actions,query_patch,effect_dedup,early_seed_guards,lhs_context_reuse,rebuild_batch,parallel_rhs,split_matches,json.loads(context_model.read_text()) if context_model else None,context_training,merge_target_index,batch_rhs,matrix_saturate,joint_polyspine,json.loads(polyspine_model.read_text()) if polyspine_model else None,polyspine_training,polyspine_full_refresh,polyspine_refresh_audit,stage_contexts,block_merge,stage_context_audit or os.environ.get("EGGFUSE_MATRIX_AUDIT")=="1",executable,scheduled_matrix,marker_program=marker_ir)
   if schedule_library is not None:
    from scheduled_matrix import bind_analyzer
    bind_analyzer(json.loads(schedule_library.read_text()),original,p,source.read_bytes().decode("utf-8"))
   p['stage_context_audit']=stage_context_audit;p['stage_context_export']=stage_context_export
   if stage_cell is not None:
    from stage_cell import load as load_stage_cell
    loaded=load_stage_cell(stage_cell,p['stage_cell']);p['stage_cell']={k:loaded[k] for k in ['contract','contract_sha256']}
   prefix=load_resume(resume,original,p,require_native_resume) if resume else []
   if max_runs is not None and max_runs<=len(prefix):raise ValueError('max-runs must exceed resumed prefix')
   del original
   (out/'program.json').write_text(json.dumps(p));(out/'kernel.metal').write_text(p['shader'])
   if context_training:
    (out/'context-training.bin').write_bytes(b'CTX1')
    atomic_json(out/'context-layout.json',{'schema':'egglog-context-features/v1','ridge':p['context_ridge'],'plans':p['query_contexts']})
   gpu=subprocess.Popen([str((build/'worker').resolve()),str((out/'program.json').resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=gl)
   protocol=read(gpu.stdout,4);assert protocol in (b'DVB4',b'DVB5',b'DVB6',b'DVB7')
   timings['noop_preflight']=0.0 if protocol in (b'DVB6',b'DVB7') else None
   timings['rhs_commit']=0.0 if protocol in (b'DVB5',b'DVB6',b'DVB7') else None
   timings['parallel_rebuild']=0.0 if protocol in (b'DVB5',b'DVB6',b'DVB7') else None
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
   def oracle_boundary(step):
    nonlocal oracle_runs
    if step.get('kind')=='joint_polyspine':return {'updated':oracle_schedule(step['schedule'])}
    if step.get('kind')!='comparison_saturate':
     oracle_runs+=1;return oracle({'op':'run','set':step['set']})
    changed=False
    while True:
     ref=oracle({'op':'run','set':step['set']});oracle_runs+=1;changed=changed or ref['updated']
     if not ref['updated']:return {'updated':changed}
   def oracle_schedule(node):
    nonlocal oracle_runs
    if 'run' in node:oracle_runs+=1;return oracle({'op':'run','set':node['run']})['updated']
    if 'sequence' in node:
     changed=False
     for child in node['sequence']:changed=oracle_schedule(child) or changed
     return changed
    if 'saturate' in node:
     changed=False
     while oracle_schedule(node['saturate']):changed=True
     return changed
    if 'repeat' in node:
     changed=False
     for _ in range(node['repeat']):
      value=oracle_schedule(node['body']);changed=changed or value
      if not value:break
     return changed
    raise ValueError('native macro schedule')
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
   matrix_calls=[];joint_calls=[];joint_plan=p['joint_polyspine'].get('program')
   def joint_closure(schedule):
    nonlocal down
    start=time.perf_counter();command(6,0);m=struct.unpack('<9I',read(gpu.stdout,36));ph=struct.unpack('<6d',read(gpu.stdout,48));index=struct.unpack('<q',read(gpu.stdout,8))[0];read(gpu.stdout,8)
    accepted,reason,n,markers,roots,new,lo,hi=struct.unpack('<8I',read(gpu.stdout,32));copied=struct.unpack('<Q',read(gpu.stdout,8))[0];stats=struct.unpack('<128q',read(gpu.stdout,1024));trace=struct.unpack('<'+'I'*(7*n),read(gpu.stdout,28*n));down+=1164+28*n
    elapsed=time.perf_counter()-start;timings['device_control_wait']+=elapsed;timings['device_gpu']+=sum(ph[:3])
    for k,v in zip(['prepare','match','commit','rhs_commit','parallel_rebuild'],ph):timings[k]+=v
    hist=[trace[i:i+7] for i in range(0,len(trace),7)]
    event={'accepted':bool(accepted),'reason':reason,'marker_bindings':markers,'root_contexts':roots,'private_phases':n,'surviving_new_rows':new,'scratch_bytes':lo+(hi<<32),'device_copy_bytes':copied,'control_seconds':elapsed,'gpu_seconds':sum(ph[:3]),'phase_history':hist,'statistics':list(stats),'global_publications':int(bool(accepted))}
    event['cell_program']={'comparison_rows_skipped':stats[120],'hits':stats[124],'operations':stats[125],'groups_skipped':stats[126],'invalidations':stats[127]}
    event['spine_program']={k:stats[38+i] for i,k in enumerate(['candidate_tuples','actions','fallback_actions','multivalue_tuples'])}
    event['cell_updates']={k:stats[25+i] for i,k in enumerate(['groups_considered','guard_pairs_eliminated','guard_pairs_checked','actions','existing_result_actions','multiple_witness_actions','owners'])}
    event['block_effects']={'enabled':p['block_merge'],'set_requests':stats[110],'set_blocks':stats[111],'sets_merged':stats[112],'peak_pending_sets':stats[113],'union_blocks':stats[114],'union_changes':stats[115],'publication_waits_removed':stats[116],'producer_audit_rows':stats[117],'function_keys_published':stats[121],'sets_carried_through_comparison':stats[123]}
    event['cost_projection']={'builds':stats[106],'reuses':stats[107],'zero_extended_ids':stats[108],'invalidation':'exact TCPair publication generation; complete correlated witnesses retained'}
    event['feature_refresh']={key:stats[96+j] for j,key in enumerate(['context_checks','base_rows_recomputed','base_rows_reused','seam_rows_recomputed','seam_rows_reused','encoding_row_lanes','ridge_row_lanes','exact_audit_rows'])}
    joint_calls.append(event);device_times.append({'kind':'joint_polyspine','set':'terms/joint','gpu_seconds':sum(ph[:3]),'prepare_seconds':ph[0],'match_seconds':ph[1],'commit_seconds':ph[2],'rhs_commit_seconds':ph[3],'parallel_rebuild_seconds':ph[4],'index_rebuilds':index})
    if not accepted:return None
    if m[4]:raise RuntimeError(f'joint Polyspine publication failed: {m[4]}')
    wm={}
    for phase,sid,epoch,*_ in hist:wm[p['sets'][sid]]=epoch if phase==2 else epoch-1
    step={'kind':'joint_polyspine','set':'terms','schedule':normalize_schedule(schedule),'ruleset_watermarks':wm,'rows':m[0],'matched':None,'updated':bool(m[3]),'epoch':m[2],'unions':m[7]};steps.append(step)
    if check_every and not check_at_end:
     ref=oracle_boundary(step)
     if ref['updated']!=step['updated']:raise AssertionError('joint/native Saturate progress mismatch')
     checkpoint()
    atomic_json(out/'progress.json',{'step':len(steps),'last':step,'joint_calls':len(joint_calls),'timings_seconds':timings})
    return step['updated']
   allowed_matrix={x['name'] for x in p['matrix_saturate']['programs']}
   allowed_comparison={x['name'] for x in p['comparison_saturate']['programs']}
   def comparison_closure(name):
    nonlocal down
    start=time.perf_counter();command(5,p['sets'].index(name));m=struct.unpack('<9I',read(gpu.stdout,36));ph=struct.unpack('<6d',read(gpu.stdout,48));index=struct.unpack('<q',read(gpu.stdout,8))[0];read(gpu.stdout,8)
    accepted,reason,iterations,unions,costs,choices,lo,hi=struct.unpack('<8I',read(gpu.stdout,32));down+=132
    elapsed=time.perf_counter()-start;timings['device_control_wait']+=elapsed;timings['device_gpu']+=sum(ph[:3])
    for k,v in zip(['prepare','match','commit','rhs_commit','parallel_rebuild'],ph):timings[k]+=v
    matrix_calls.append({'kind':'comparison_saturate','set':name,'accepted':bool(accepted),'reason':reason,'local_iterations':iterations,'local_unions':unions,'cost_rows':costs,'markers':choices,'logical_runs':None,'published_rows':0,'scratch_bytes':lo+(hi<<32),'native_match_counts_checked':False,'gpu_seconds':sum(ph[:3]),'control_seconds':elapsed})
    device_times.append({'kind':'comparison_saturate','set':name,'gpu_seconds':sum(ph[:3]),'prepare_seconds':ph[0],'match_seconds':ph[1],'commit_seconds':ph[2],'rhs_commit_seconds':ph[3],'parallel_rebuild_seconds':ph[4],'index_rebuilds':index})
    if not accepted:return None
    if m[4]:raise RuntimeError(f'comparison closure publication/rebuild failed: {m[4]}')
    # A macro is one public Saturate operation. Local union iterations are not
    # original Run records or logical rule matches, so no fabricated count.
    step={'kind':'comparison_saturate','set':name,'rows':m[0],'matched':None,'updated':bool(m[3]),'epoch':m[2],'unions':m[7],'local_iterations':iterations}
    steps.append(step)
    if check_every and not check_at_end:
     ref=oracle_boundary(step)
     if ref['updated']!=step['updated']:raise AssertionError('comparison/native Saturate progress mismatch')
     checkpoint()
    atomic_json(out/'progress.json',{'step':len(steps),'last':step,'matrix_calls':len(matrix_calls),'timings_seconds':timings})
    return step['updated']
   def matrix_closure(name):
    nonlocal down,oracle_runs
    start=time.perf_counter();command(4,p['sets'].index(name));m=struct.unpack('<9I',read(gpu.stdout,36));ph=struct.unpack('<6d',read(gpu.stdout,48));index=struct.unpack('<q',read(gpu.stdout,8))[0];read(gpu.stdout,8)
    accepted,reason,n,markers,new,lo,hi,_=struct.unpack('<8I',read(gpu.stdout,32));history=struct.unpack('<'+'I'*(2*n),read(gpu.stdout,8*n));down+=132+8*n
    control_seconds=time.perf_counter()-start
    timings['device_control_wait']+=control_seconds;timings['device_gpu']+=sum(ph[:3])
    for k,v in zip(['prepare','match','commit'],ph):timings[k]+=v
    timings['rhs_commit']+=ph[3]
    event={'set':name,'accepted':bool(accepted),'reason':reason,'logical_runs':n,'markers':markers,'published_rows':new,'scratch_bytes':lo+(hi<<32),'history':list(zip(history[::2],history[1::2])),'native_match_counts_checked':bool(check_every or check_at_end),'gpu_seconds':sum(ph[:3]),'control_seconds':control_seconds}
    matrix_calls.append(event)
    device_times.append({'set':name,'kind':'matrix_saturate','gpu_seconds':sum(ph[:3]),'prepare_seconds':ph[0],'match_seconds':ph[1],'commit_seconds':ph[2],'rhs_commit_seconds':ph[3],'parallel_rebuild_seconds':0,'index_rebuilds':index,'rhs_noops_skipped':0,'duplicate_effects_skipped':0})
    if not accepted:return None
    if m[4]:raise RuntimeError('matrix publication failed')
    rows=m[0]-new;epoch=m[2]-n;changed=False
    for count,matched in zip(history[::2],history[1::2]):
     rows+=count;epoch+=1;changed|=count>0
     steps.append({'set':name,'rows':rows,'matched':matched,'updated':count>0,'epoch':epoch,'unions':m[7],'matrix_macro':len(matrix_calls)-1})
     if check_every and not check_at_end:
      ref=oracle({'op':'run','set':name});oracle_runs+=1
      if 'logical_matches' not in ref:event['native_match_counts_checked']=False
      if ref['updated']!=(count>0) or ('logical_matches' in ref and ref['logical_matches']!=matched):raise AssertionError(f'matrix/native logical Run history mismatch: {ref} versus {(count,matched)}')
    if check_every and not check_at_end:checkpoint()
    atomic_json(out/'progress.json',{'step':len(steps),'last':steps[-1],'matrix_calls':len(matrix_calls),'timings_seconds':timings})
    return changed
   scheduled_calls=[]
   scheduled_plans={json.dumps(x['schedule'],sort_keys=True,separators=(',',':')):x for x in (p.get('scheduled_matrix') or {}).get('programs',[])}
   def scheduled_closure(plan):
    nonlocal down,oracle_runs
    start=time.perf_counter();command(8,plan['index']);m=struct.unpack('<9I',read(gpu.stdout,36));ph=struct.unpack('<3d',read(gpu.stdout,24))
    accepted,reason,n,markers,lo,hi,forwarded=struct.unpack('<7I',read(gpu.stdout,28));words=struct.unpack('<'+'I'*(8*n),read(gpu.stdout,32*n));down+=88+32*n
    elapsed=time.perf_counter()-start;timings['device_control_wait']+=elapsed;timings['device_gpu']+=sum(ph)
    for k,v in zip(['prepare','match','commit'],ph):timings[k]+=v
    timings['rhs_commit']+=ph[2]
    event={'program':plan['index'],'accepted':bool(accepted),'reason':reason,'rulesets':plan['rulesets'],'logical_runs':n,'markers_acquired_once':markers,'scratch_bytes':lo+(hi<<32),'gpu_seconds':sum(ph),'control_seconds':elapsed,'global_publications':int(bool(accepted)),'context_reacquisitions':0}
    scheduled_calls.append(event)
    device_times.append({'kind':'scheduled_matrix','set':'/'.join(plan['rulesets']),'gpu_seconds':sum(ph),'prepare_seconds':ph[0],'match_seconds':ph[1],'commit_seconds':ph[2],'rhs_commit_seconds':ph[2],'parallel_rebuild_seconds':0,'index_rebuilds':0})
    if not accepted:return None
    if m[4]:raise RuntimeError('scheduled Cell publication failed')
    changed=False;new=0
    for at in range(0,len(words),8):
     sid,added,matched,epoch,rows,pc,depth,iteration=words[at:at+8];name=p['sets'][sid];changed|=added>0;new+=added
     steps.append({'set':name,'rows':rows,'matched':matched,'updated':added>0,'epoch':epoch,'unions':m[7],'scheduled_program':plan['index'],'program_counter':pc,'loop_depth':depth,'loop_iteration':iteration})
     if check_every and not check_at_end:
      ref=oracle({'op':'run','set':name});oracle_runs+=1
      if ref['updated']!=(added>0) or ('logical_matches' in ref and ref['logical_matches']!=matched):raise AssertionError(f'scheduled/native Run differs: {ref} versus {(added,matched)}')
    event['published_rows']=new;event['patch_operand_reads']=forwarded
    if check_every and not check_at_end:checkpoint()
    return changed
   class PrefixComplete(Exception):pass
   def advance(s):
    nonlocal down,oracle_runs
    if s.get('until_run'):
     advance({'run':s['until_run'],'until_probe':True})
     if steps[-1]['matched']>0:return False
     s={k:v for k,v in s.items() if k!='until_run'}
    sk=json.dumps(s,sort_keys=True,separators=(',',':'))
    if scheduled_matrix and max_runs is None and not prefix and sk in scheduled_plans:
     done=scheduled_closure(scheduled_plans[sk])
     if done is not None:return done
    if len(steps)<len(prefix) and prefix[len(steps)].get('kind')=='joint_polyspine' and normalize_schedule(s)==prefix[len(steps)]['schedule']:
     expected=prefix[len(steps)];steps.append(expected)
     if check_every and not check_at_end:
      if oracle_boundary(expected)['updated']!=expected['updated']:raise AssertionError('resume native joint scope differs')
      if len(steps)==len(prefix):checkpoint()
     return expected['updated']
    if joint_polyspine and joint_plan and max_runs is None and not prefix and normalize_schedule(s)==joint_plan['schedule']:
     value=joint_closure(s)
     if value is not None:return value
    if 'run' in s:
     if len(steps)<len(prefix):
      expected=prefix[len(steps)]
      if expected['set']!=s['run']:raise ValueError('resume schedule differs')
      steps.append(expected)
      if check_every and not check_at_end:
       ref=oracle_boundary(expected)
       if ref['updated']!=expected['updated']:raise AssertionError('resume native prefix differs')
       if len(steps)==len(prefix):checkpoint()
      return expected['updated']
     if max_runs is not None and len(steps)>=max_runs:raise PrefixComplete()
     setid=p['sets'].index(s['run']);
     if os.environ.get('EGG_DEVICE_PHASE_LOG')=='1':atomic_json(out/'pending-command.json',{'step':len(steps)+1,'set':s['run']})
     t=time.perf_counter();command(1,setid);m=struct.unpack('<9I',read(gpu.stdout,36));nphases=6 if protocol in (b'DVB6',b'DVB7') else 5 if protocol==b'DVB5' else 3;phase_seconds=struct.unpack('<'+'d'*nphases,read(gpu.stdout,nphases*8));gpu_seconds=sum(phase_seconds[:3]);index_rebuilds=struct.unpack('<q',read(gpu.stdout,8))[0];down+=44+8*nphases
     skipped=struct.unpack('<I',read(gpu.stdout,4))[0] if protocol in (b'DVB6',b'DVB7') else None
     if protocol in (b'DVB6',b'DVB7'):down+=4
     duplicate_effects=struct.unpack('<I',read(gpu.stdout,4))[0] if protocol==b'DVB7' else None
     if protocol==b'DVB7':down+=4
     for name,value in zip(['prepare','match','commit'],phase_seconds):timings[name]+=value
     timings['device_control_wait']+=time.perf_counter()-t;timings['device_gpu']+=gpu_seconds;device_times.append({'set':s['run'],'gpu_seconds':gpu_seconds,'prepare_seconds':phase_seconds[0],'match_seconds':phase_seconds[1],'commit_seconds':phase_seconds[2],'index_rebuilds':index_rebuilds})
     if protocol in (b'DVB5',b'DVB6',b'DVB7'):
      for name,value in zip(['rhs_commit','parallel_rebuild'],phase_seconds[3:5]):
       timings[name]+=value;device_times[-1][name+'_seconds']=value
     if s.get('assert') and not m[5] and not m[4]:raise AssertionError('GPU source check failed')
     if protocol in (b'DVB6',b'DVB7'):
      separate=phase_seconds[5]>=0
      timings['noop_preflight']=(timings['noop_preflight']+phase_seconds[5]) if separate and timings['noop_preflight'] is not None else None
      device_times[-1]['noop_preflight_seconds']=phase_seconds[5] if separate else None
      device_times[-1]['rhs_commit_includes_preflight']=not separate
      device_times[-1]['rhs_noops_skipped']=skipped
     if protocol==b'DVB7':device_times[-1]['duplicate_effects_skipped']=duplicate_effects
     if m[4]:raise RuntimeError(f'GPU incomplete, error={m[4]} (1 storage capacity, 2 match capacity, 3 missing lookup, 4 conflicting function, 5 overflow, 6 epoch exhaustion, 7 ambiguous ordered merge)')
     if context_training:
      command(3);header=read(gpu.stdout,16);n,width,epoch,overflow=struct.unpack('<4I',header)
      if epoch!=m[2]:raise AssertionError('stale context training frame')
      with (out/'context-training.bin').open('ab') as dataset:dataset.write(header);dataset.write(read(gpu.stdout,n*(width+2)*4))
     steps.append({'set':s['run'],'rows':m[0],'matched':m[5],'updated':bool(m[3]),'epoch':m[2],'unions':m[7],**({'kind':'until_probe'} if s.get('until_probe') else {})})
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
     xs=s['sequence'];i=0
     while i<len(xs):
      end=i+1;chosen=None
      if scheduled_matrix and max_runs is None and not prefix:
       for j in range(i+2,len(xs)+1):
        k=json.dumps({'sequence':xs[i:j]},sort_keys=True,separators=(',',':'))
        if k in scheduled_plans:end=j;chosen=scheduled_plans[k]
      if chosen is not None:
       done=scheduled_closure(chosen)
       if done is not None:changed=done or changed;i=end;continue
      changed=advance(xs[i]) or changed;i+=1
     return changed
    if 'repeat' in s:
     changed=False
     for _ in range(s['repeat']):
      updated=advance(s['body']);changed=updated or changed
      if not updated:break
     return changed
    if 'saturate' in s:
     body=s['saturate']
     while 'sequence' in body and len(body['sequence'])==1:body=body['sequence'][0]
     single=body.get('run')
     if single is not None and len(steps)<len(prefix) and prefix[len(steps)].get('kind')=='comparison_saturate':
      expected=prefix[len(steps)]
      if single!=expected['set']:raise ValueError('resume Saturate scope differs')
      steps.append(expected)
      if check_every and not check_at_end:
       ref=oracle_boundary(expected)
       if ref['updated']!=expected['updated']:raise AssertionError('resume native Saturate differs')
       if len(steps)==len(prefix):checkpoint()
      return expected['updated']
     if matrix_saturate and single in allowed_comparison and max_runs is None and not prefix:
      result=comparison_closure(single)
      if result is not None:return result
     if matrix_saturate and single in allowed_matrix and max_runs is None and not prefix:
      result=matrix_closure(single)
      if result is not None:return result
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
     ref=oracle_boundary(step)
     if ref['updated']!=step['updated']:raise AssertionError('post-run CPU/GPU progress mismatch')
     if 'matrix_macro' in step:
      event=matrix_calls[step['matrix_macro']]
      if 'logical_matches' in ref:
       if ref['logical_matches']!=step['matched']:raise AssertionError('post-run matrix/native match counts differ')
      else:event['native_match_counts_checked']=False
    checkpoint()
   if check_every and not check_at_end and (not checks or checks[-1]['step']!=len(steps)):checkpoint()
   if control is not None:
    key=lambda ss:[(s['set'],s['updated']) for s in ss]
    if key(steps)!=key(control['steps']) and not (complete and control['complete_schedule'] and not prefix and any(s.get('kind') in ['comparison_saturate','joint_polyspine'] for s in steps)):
     raise AssertionError('stored reference schedule differs')
    checkpoint()
   if save_final_state and (not checks or checks[-1]['step']!=len(steps)):read_gpu_state()
   elapsed=time.perf_counter()-started
   command(0);gpu.wait(timeout=30)
   if native.poll() is None:
    native.stdin.write('{"op":"quit"}\n');native.stdin.flush();native.wait(timeout=30)
   clean_timing=not stage_context_audit and not stage_context_export and not context_training and not polyspine_training and not polyspine_refresh_audit and (not check_every or check_at_end) and not snapshot_every and os.environ.get('EGG_DEVICE_PHASE_LOG')!='1' and os.environ.get('MTL_DEBUG_LAYER')!='1' and os.environ.get('EGGFUSE_MATRIX_AUDIT')!='1'
   allocated=re.search(r'device allocated_bytes=(\d+)',(out/'gpu.log').read_text())
   refresh=re.search(r'device index_refresh full=(\d+) patch=(\d+)',(out/'gpu.log').read_text())
   rb_counts=re.search(r'rebuild iterations=(\d+) waits=(\d+)',(out/'gpu.log').read_text())
   ctx_counts=re.search(r'contexts emitted=(\d+) fallback_runs=(\d+)',(out/'gpu.log').read_text())
   batch_counts=re.search(r'batch_rhs proposals=(\d+) new_rows=(\d+) fallback_segments=(\d+) parallel_segments=(\d+)',(out/'gpu.log').read_text())
   result={'scheduled_matrix':{'enabled':scheduled_matrix,'program':p.get('scheduled_matrix'),'calls':scheduled_calls},'cell_program':p['cell_program'],'block_merge':p['block_merge'],'joint_polyspine':{'enabled':joint_polyspine,'program':p['joint_polyspine'],'calls':joint_calls,'training':polyspine_training,'feature_refresh':'full' if polyspine_full_refresh else 'incremental','feature_refresh_audit':polyspine_refresh_audit},'matrix_saturate':{'enabled':matrix_saturate,'programs':p['matrix_saturate']['programs'],'comparison_programs':p['comparison_saturate']['programs'],'comparison_rejected':p['comparison_saturate']['rejected'],'rejected':p['matrix_saturate']['rejected'],'calls':matrix_calls,'check_scope':'ordinary Run boundaries and complete Saturate boundaries'},'batch_rhs':{'enabled':batch_rhs,'eligible_rules':p['batch_rhs']['eligible_rules'],'proposals':int(batch_counts.group(1)),'new_rows':int(batch_counts.group(2)),'fallback_segments':int(batch_counts.group(3)),'parallel_segments':int(batch_counts.group(4))} if batch_counts else None,'merge_target_index':merge_target_index,'merge_target_tables':p['merge_target_tables'],'context_ridge':p['context_ridge'],'context_training':context_training,'contexts':{'emitted':int(ctx_counts.group(1)),'fallback_runs':int(ctx_counts.group(2))} if ctx_counts else None,'split_matches':split_matches,'query_context_plans':len(p['query_contexts']),'parallel_rhs':parallel_rhs,'parallel_rhs_rules':p['parallel_rhs_rules'],'rebuild_batch':rebuild_batch,'rebuild_dispatches':{'iterations':int(rb_counts.group(1)) if rebuild_batch>1 else int(rb_counts.group(2)),'host_waits':int(rb_counts.group(2))} if rb_counts else None,'lhs_context_reuse':lhs_context_reuse,'rhs_context_reuses':len(p['rhs_context_reuses']),'early_seed_guards':early_seed_guards,'effect_dedup':effect_dedup,'duplicate_effects_skipped':sum(v.get('duplicate_effects_skipped',0) for v in device_times),'query_index_refresh':{'full':int(refresh.group(1)),'patch':int(refresh.group(2))} if refresh else None,'query_patch':query_patch,'direct_actions':direct_actions,'noop_preflight':noop_preflight,'rhs_noops_skipped':sum(v.get('rhs_noops_skipped',0) for v in device_times),'seed_dispatch':seed_dispatch,'query_radix_digit_bits':radix_digit_bits,'all_index_columns':all_index_columns,'parallel_merge_index':parallel_merge_index,'setup_seconds':setup_seconds,'schedule_seconds':schedule_seconds,'inference_seconds':schedule_seconds if clean_timing else None,'clean_timing':clean_timing,'gpu_allocated_bytes_at_start':int(allocated.group(1)) if allocated else None,'indexed_subsume':indexed_subsume,'resume_validation':'native' if require_native_resume else 'GPU canonical state only','snapshot_every':snapshot_every,'parallel_match':parallel_match,'parallel_prepare':parallel_prepare,'parallel_rebuild':parallel_rebuild,'state_export_requested':save_final_state,'phase_logging':os.environ.get('EGG_DEVICE_PHASE_LOG')=='1','verify_against':str(verify_against.resolve()) if verify_against else None,'resume_directory':str(resume.resolve()) if resume else None,'resumed_prefix_runs':len(prefix),'incremental_queries':incremental_queries,'delta_queries':delta_queries,'deferred_rulesets':p['deferred_rulesets'],'timings_seconds':timings,'device_command_times':device_times,'complete_schedule':complete,'requested_max_runs':max_runs,'scope':'native-compiled .egg, experimental GPU-owned execution backend; consult clean_timing, completeness and validation scope before using timings','steps':steps,'checks':checks,'checked':bool(check_every or check_at_end or verify_against),'check_mode':'stored' if verify_against else 'final' if check_at_end else ('periodic' if check_every else 'off'),'native_oracle_runs':oracle_runs,'run_control_upload_bytes':8*len(device_times),'run_status_download_bytes':down,'checkpoint_bytes':snapshot_bytes,'intermediate_binding_download_bytes':0,'cpu_rhs_calls':0,'cpu_rebuild_calls_outside_oracle':0,'wall_seconds_including_debug_checks':elapsed,'initial_row_upload_bytes':initial_bytes,'native_retired_before_gpu_runs':not bool(check_every or check_at_end),'oracle_starts_after_gpu':check_at_end,'control_upload_bytes':up,'assertion_tables':p.get('assertion_tables',[]),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'build':json.loads((build/'build.json').read_text()),'program_sha256':hashlib.sha256((out/'program.json').read_bytes()).hexdigest()}
   fused_counts=re.findall(r'joint_fused_prepare phases=(\d+) workspace_bytes=(\d+)',(out/'gpu.log').read_text())
   run_encoding_counts=re.findall(r'shared_run_encoding dispatches=(\d+) encoders=(\d+)',(out/'gpu.log').read_text())
   result['shared_run_encoding']={'enabled':os.environ.get('EGGFUSE_SHARED_ENCODER')=='1','dispatches':sum(int(x[0]) for x in run_encoding_counts),'encoders':sum(int(x[1]) for x in run_encoding_counts),'scope':'ordinary preparation, binding sorting and rebuild; not additional Cell proof coverage'}
   shared_counts=re.findall(r'joint_shared_encoding dispatches=(\d+) encoders=(\d+)',(out/'gpu.log').read_text())
   result['joint_execution']={'shared_encoding':os.environ.get('EGGFUSE_JOINT_SHARED_ENCODER')=='1','shared_dispatches':sum(int(x[0]) for x in shared_counts),'shared_encoders':sum(int(x[1]) for x in shared_counts),'fused_prepare':os.environ.get('EGGFUSE_JOINT_FUSE_PREPARE')=='1','local_sort':os.environ.get('EGGFUSE_JOINT_LOCAL_SORT')=='1','removed_host_fences':sum(int(x[0]) for x in fused_counts),'peak_job_workspace_bytes':max([int(x[1]) for x in fused_counts]+[0]),'phase_timing_note':'fused preparation is included in the joint match phase; use joint totals for comparison'}
   if stage_contexts:
    stage_path=out/'stage-contexts.json'
    stages=json.loads(stage_path.read_text())
    result['stage_contexts']=stages;result['stage_cell_contract_sha256']=p['stage_cell']['contract_sha256']
    result['loaded_stage_cell']=str(stage_cell) if stage_cell else None
    if bake_stage_cell is not None:
     from stage_cell import save as save_stage_cell
     save_stage_cell(bake_stage_cell,p['stage_cell'],result,stages);result['baked_stage_cell']=str(bake_stage_cell)
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
 a=argparse.ArgumentParser();a.add_argument('--compiled-program',type=Path);a.add_argument('--marker-program',type=Path);a.add_argument('--generic-cell-library',type=Path,help='execute analyzer-exported original schedule with the generic private-context interpreter');a.add_argument('--scheduled-matrix',action='store_true');a.add_argument('--schedule-library',type=Path);a.add_argument('--cell-program',type=Path);bm=a.add_mutually_exclusive_group();bm.add_argument('--block-merge',action='store_true',dest='block_merge',default=None);bm.add_argument('--eager-stage-merge',action='store_false',dest='block_merge');a.add_argument('--stage-contexts',action='store_true');a.add_argument('--bake-stage-cell',type=Path);a.add_argument('--stage-cell',type=Path);a.add_argument('--audit-stage-contexts',action='store_true');a.add_argument('--export-stage-contexts',action='store_true');a.add_argument('--full-polyspine-refresh',action='store_true',help='disable phase-local feature reuse for ablation');a.add_argument('--audit-polyspine-refresh',action='store_true',help='compare cached features and scores with full GPU recomputation each private phase');a.add_argument('--joint-polyspine',action='store_true');a.add_argument('--polyspine-model',type=Path);a.add_argument('--polyspine-training',action='store_true');a.add_argument('--matrix-saturate',action='store_true',help='execute certified predicate/equality Saturate blocks in resident matrices');a.add_argument('--batch-rhs',action='store_true',help='parallel independent-constructor publication in original rule-order segments');a.add_argument('--merge-target-index',action='store_true',help='index only statically reachable constructor tables while resuming merge callbacks');a.add_argument('--context-model',type=Path,help='optional typed ridge continuation model');a.add_argument('--context-training',action='store_true',help='diagnostic GPU context feature/label export; excluded from timings');a.add_argument('--split-matches',action='store_true',help='split branching queries into snapshot-versioned binding contexts');a.add_argument('--parallel-rhs',action='store_true',help='parallel execution of certified allocation-free commuting RHS rulesets');a.add_argument('--rebuild-batch',type=int,default=1,help='rebuild iterations submitted per host wait; not a convergence limit');a.add_argument('--source',type=Path,required=True);a.add_argument('--build',type=Path,required=True);a.add_argument('--output',type=Path,required=True);a.add_argument('--check-every',type=int,default=1,help='0 disables CPU execution; positive N verifies every N Run boundaries');a.add_argument('--check-at-end',action='store_true',help='replay native only after all GPU Run commands finish');a.add_argument('--capacity',type=int,default=4096);a.add_argument('--match-capacity',type=int,default=16384);a.add_argument('--no-lhs-context-reuse',action='store_true',help='diagnostic: repeat RHS lookups already witnessed by LHS');a.add_argument('--early-seed-guards',action='store_true',help='experimental: hoist seed timestamp guards to callers');a.add_argument('--late-seed-guards',action='store_true',help='diagnostic: check seed timestamps inside each query');a.add_argument('--effect-dedup',action='store_true',help='experimental: merge identical idempotent RHS recipes');a.add_argument('--no-effect-dedup',action='store_true',help='diagnostic: keep repeated idempotent RHS submissions');a.add_argument('--full-query-updates',action='store_true',help='diagnostic: rebuild all indexes after any existing value update');a.add_argument('--bytecode-actions',action='store_true',help='diagnostic: interpret every RHS instruction');a.add_argument('--no-noop-preflight',action='store_true',help='diagnostic: serially execute even stable empty RHS');a.add_argument('--flat-dispatch',action='store_true',help='diagnostic: call every query for every seed row');a.add_argument('--radix-digit-bits',type=int,choices=[1,4],default=4);a.add_argument('--all-index-columns',action='store_true',help='diagnostic indexes on every column');a.add_argument('--serial-merge-index',action='store_true',help='diagnostic index rebuild inside merge callbacks');a.add_argument('--scan-subsume',action='store_true',help='diagnostic full-table subsumption');a.add_argument('--resume-gpu',type=Path,help='restore canonical GPU state without claiming native equality');a.add_argument('--snapshot-every',type=int,default=0,help='diagnostic GPU-only checkpoints');a.add_argument('--serial-match',action='store_true',help='diagnostic serial matcher');a.add_argument('--serial-prepare',action='store_true',help='diagnostic ordered-chain preparation');a.add_argument('--serial-rebuild',action='store_true',help='diagnostic serial rebuild reference');a.add_argument('--save-state',action='store_true',help='export final GPU state without requiring native execution');a.add_argument('--verify-against',type=Path,help='reuse an exact native final checkpoint; no native Run replay');a.add_argument('--no-delta-queries',action='store_true',help='diagnostic: filter changed bindings only after joining');a.add_argument('--resume',type=Path,help='resume from a native-verified checkpoint; not an end-to-end timing run');a.add_argument('--rebuild-query-index',action='store_true',help='diagnostic: rebuild snapshot indexes on every Run');a.add_argument('--eager-unions',action='store_true',help='diagnostic legacy mode; may differ semantically from native');a.add_argument('--max-runs',type=int,help='diagnostic prefix only, reported as incomplete');x=a.parse_args()
 if x.generic_cell_library:
  if any([x.compiled_program,x.resume,x.resume_gpu,x.verify_against,x.max_runs,x.cell_program,x.scheduled_matrix,x.schedule_library,x.stage_contexts,x.joint_polyspine,x.polyspine_model,x.split_matches,x.context_model]):a.error('generic Cell execution is a separate exact backend mode; do not combine execution strategies')
  from run_generic_cells import run_source
  print(json.dumps(run_source(x.source,x.generic_cell_library,x.build,x.output,x.check_every>0 or x.check_at_end,x.capacity,x.match_capacity),indent=2));return
 if x.resume and x.resume_gpu:a.error('choose one resume mode')
 if x.snapshot_every<0:a.error('snapshot interval must be nonnegative')
 if x.check_every<0:a.error('negative checkpoint interval')
 if x.max_runs is not None and x.max_runs<1:a.error('max-runs must be positive')
 print(json.dumps(run(x.source,x.build,x.output,x.check_every,x.capacity,x.match_capacity,x.check_at_end,x.max_runs,not x.eager_unions,not x.rebuild_query_index,x.resume or x.resume_gpu,not x.no_delta_queries,x.verify_against,x.save_state,not x.serial_rebuild,not x.serial_prepare,not x.serial_match,x.resume_gpu is None,x.snapshot_every,not x.scan_subsume,not x.serial_merge_index,x.all_index_columns,x.radix_digit_bits,not x.flat_dispatch,not x.no_noop_preflight,not x.bytecode_actions,not x.full_query_updates,x.effect_dedup and not x.no_effect_dedup,x.early_seed_guards and not x.late_seed_guards,not x.no_lhs_context_reuse,x.rebuild_batch,x.parallel_rhs,x.split_matches,x.context_model,x.context_training,x.merge_target_index,x.batch_rhs,x.matrix_saturate,x.joint_polyspine,x.polyspine_model,x.polyspine_training,x.full_polyspine_refresh,x.audit_polyspine_refresh,x.stage_contexts,x.bake_stage_cell,x.stage_cell,x.audit_stage_contexts,x.export_stage_contexts,x.block_merge,x.cell_program,x.scheduled_matrix,x.schedule_library,x.marker_program,x.compiled_program),indent=2))
if __name__=='__main__':main()

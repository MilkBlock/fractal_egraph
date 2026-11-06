from pathlib import Path
import subprocess,os,json,time,tempfile,hashlib,re,statistics,sys
P=Path(__file__).resolve().parent
R=Path('/Users/mineralsteins/Repos/egg_related/eggfuse-report-pr/docs/papers/eggcc-cpu-techreport')
cases=json.load(open(P/'capture/manifest.json'))['cases']
historical={(x['case'],x['pass']):x for x in map(json.loads,(R/'suite.jsonl').read_text().splitlines())}
env={k:v for k,v in os.environ.items() if not k.startswith(('CELL_CPU_','RAYON_','RUSTFLAGS','CARGO_PROFILE_','LLVM_PROFILE_','EGGLOG_COUNT_','EGG_LAYOUT_'))}
env['RAYON_NUM_THREADS']='1'
def sha(p):
 with open(p,'rb')as f:return hashlib.file_digest(f,'sha256').hexdigest()
def call(args):
 t=time.monotonic();r=subprocess.run([str(a)for a in args],capture_output=True,env=env,timeout=360)
 if r.returncode:raise RuntimeError(r.stderr.decode(errors='replace')[-1800:])
 return r,time.monotonic()-t
def one(index,c,phase,reps):
 name=c['source'].split('/benchmarks/passing/')[1];meta=c['passes'][phase-1]
 folder=P/'capture'/Path(c['directory']).name;source=folder/Path(meta['input']).name
 assert sha(source)==meta['input_sha256']
 row=dict(case=name,directory=c['directory'],pass_number=phase,source_sha256=c['source_sha256'],flat_sha256=sha(source),samples=[],status='pending')
 with tempfile.TemporaryDirectory(dir=P/'scratch')as tmp:
  w=Path(tmp);begin=time.monotonic()
  try:
   call([P/'compiler','compile','--source',source,'--bake-source',P/'bake.egg','--output',w/'compiled'])
   r,_=call([P/'transfer',P/'donor.json',w/'compiled/program.json',w/'ready.json'])
   row['units']=int(r.stdout.strip());row['prepare_s']=time.monotonic()-begin
   program=json.load(open(w/'ready.json'));row['program_sha256']=program['sha256'];row['recipe_sha256']=program['recipe']['sha256']
  except Exception as e:
   row['status']='preparation-error';row['error']=str(e);return row
  for iteration in range(reps):
   sample={'i':iteration+1}
   order=['native','cell']if(index+phase+iteration)%2==0 else['cell','native']
   sample['order']=order
   for arm in order:
    try:
     if arm=='native':
      r,wall=call(['/usr/bin/time','-l',P/'native',source]);text=r.stdout.decode()
      value=re.findall(r' ([\d.]+) ms',text)
      assert value,text[-500:]
      sample['native_ms']=float(value[-1]);sample['native_cli_ms']=wall*1000
      sample['native_rss_bytes']=int(re.search(r'(\d+)\s+maximum resident set size',r.stderr.decode())[1])
     else:
      out=w/f'run-{iteration}';r,wall=call([P/'cell','--program',w/'ready.json','--output',out])
      data=json.load(open(out/'report.json'));assert data['status']=='complete'
      sample.update(cell_ms=data['inference_seconds']*1000,cell_cli_ms=wall*1000,cell_rss_bytes=data['peak_rss_through_inference_bytes'])
      graph_hash=sha(out/'state.json');(out/'state.json').unlink()
      keys=['ordinal','ruleset','path','iterations','epoch','logical_matches','executed_bindings','changed']
      schedule_hash=hashlib.sha256(json.dumps([[s[k]for k in keys]for s in data['steps']]).encode()).hexdigest()
      for key,val in dict(state_sha256=graph_hash,schedule_sha256=schedule_hash,runs=len(data['steps']),bindings=data['stats']['bindings'],resumed=data['stage_spine']['stats']['resumed_steps']).items():
       if key in row:assert row[key]==val,(key,row[key],val)
       row[key]=val
      row['matches_historical_cell_endpoint']=graph_hash==historical[name,phase]['state_sha256']
    except Exception as e:
     sample[arm+'_error']=str(e)
   row['samples'].append(sample)
  good=all('native_ms'in s and 'cell_ms'in s for s in row['samples'])
  row['status']='complete'if good else'run-error'
  if good:
   for metric in ['native_ms','cell_ms','native_cli_ms','cell_cli_ms','native_rss_bytes','cell_rss_bytes']:
    row[metric]=statistics.median(s[metric]for s in row['samples'])
   row['speedup']=row['native_ms']/row['cell_ms']
  return row
mode=sys.argv[1] if len(sys.argv)>1 else 'pilot'
output=P/('pilot.jsonl'if mode=='pilot'else'suite-current.jsonl')
done=set()
if output.exists():done={(v['case'],v['pass_number'])for v in map(json.loads,output.read_text().splitlines())}
for phase in ([3]if mode=='pilot'else[3,1,2]):
 for i,c in enumerate(cases):
  if mode=='pilot' and not c['directory'].startswith('032-'):continue
  name=c['source'].split('/benchmarks/passing/')[1]
  if (name,phase)in done:continue
  row=one(i,c,phase,1 if mode=='pilot'else 3)
  with output.open('a')as f:print(json.dumps(row),file=f)
  print(i,phase,name,row['status'],round(row.get('native_ms',0),3),round(row.get('cell_ms',0),3),row.get('matches_historical_cell_endpoint'),row.get('error','')[:150],flush=True)

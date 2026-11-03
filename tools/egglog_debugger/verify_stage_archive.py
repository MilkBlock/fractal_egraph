"""Native archive observation/isolation integration checks; no browser needed."""
import argparse,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT));sys.path.insert(0,str(Path(__file__).parent))
from eggfuse.compiler.targets.cpu import lower
from eggfuse.engine.cpu import binary,compare_native
from stage_sampler import Archive,lines,build_sample
CASES={
 'math':(ROOT/'tools/egglog_debugger/fixtures/stage-math.egg').read_text(),
 'merge':(ROOT/'tools/egglog_debugger/fixtures/stage-merge.egg').read_text(),
 'visibility':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(N 0) (run-schedule (repeat 1 (run a) (run b)))''',
 'set-alias':'''(sort E) (constructor N (i64) E) (sort ES (Set E)) (constructor Box (ES) E)
(ruleset s) (rule ((= x (N 1)) (= y (N 2))) ((union x y)) :ruleset s)
(Box (set-of (N 1) (N 2))) (run s 1)''',
 'delete':'''(datatype E (N i64) (P E)) (ruleset a) (ruleset hide)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((subsume (P x))) :ruleset hide)
(N 0) (run a 1) (run hide 1) (delete (N 0)) (run a 1)''',
}
def normalized(v):
 v=dict(v);v.pop('display',None)
 if 'value' in v and v['sort'] in ('i64','bool','Unit','f64'):v['value']=int(v['value'])
 if 'elements' in v:v['elements']=[normalized(x) for x in v['elements']]
 return v

def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--cpu-bin',type=Path);a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
 engine=a.cpu_bin.resolve() if a.cpu_bin else binary();report={}
 for name,source in CASES.items():
  d=a.output/name;d.mkdir();src=d/'source.egg';src.write_text(source)
  subprocess.run([str(engine),'--source',str(src),'--prepare',str(d/'core.json')],check=True)
  core=json.loads((d/'core.json').read_text());(d/'plan.json').write_text(json.dumps(lower(core)))
  for mode in ('staged','native'):
   cmd=[str(engine),'--source',str(src),'--mode',mode,'--output',str(d/mode)]
   if mode=='staged':cmd+=['--plan',str(d/'plan.json'),'--stage-archive',str(d/'archive')]
   with (d/(mode+'.log')).open('w') as f:subprocess.run(cmd,stdout=f,stderr=subprocess.STDOUT,check=True)
  actual=json.loads((d/'staged/state.json').read_text());reference=json.loads((d/'native/state.json').read_text());proof=compare_native(core,actual,reference)
  stages=lines(d/'archive/stages.ndjson');replay=Archive(d/'archive');last=replay.seek(stages[-1]['after'])
  decoded=[{'table':core['tables'][t]['name'],'values':[normalized(last['values'][v]) for v in vs],'subsumed':h} for t,vs,h in last['rows'].values()]
  compare_native(core,{'rows':decoded,'anchors':actual['anchors']},reference)
  if name=='visibility':
   def names(frame):return {core['tables'][t]['name'] for t,_,_ in replay.seek(frame)['rows'].values()}
   assert 'Q' not in names(stages[1]['after']);assert 'Q' not in names(stages[2]['before']);assert 'Q' in names(stages[2]['after'])
  if name=='set-alias':
   before,after=replay.pair(stages[1]);sample=build_sample(core,before,after,stages[1],{'focus':'all','sort':'E','depth':3,'budget':20})
   assert sample['stats']['changed_in_view']==0,sample
  for stage in stages:
   before,after=replay.pair(stage)
   for seed in (0,7):
    sample=build_sample(core,before,after,stage,{'focus':'all','budget':8,'seed':seed})
    if not sample['empty']:assert sample['stats']['visible_facts']<=8
  size=sum(p.stat().st_size for p in (d/'archive').iterdir())
  report[name]={'stages':len(stages)-1,'final_rows':proof['rows'],'archive_bytes':size,'exact_native_graph':True}
  print(name,report[name],flush=True)
 (a.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
if __name__=='__main__':main()

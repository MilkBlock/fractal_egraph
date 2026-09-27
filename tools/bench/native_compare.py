#!/usr/bin/env python3
"""Same public-API harness, pristine tag versus current native engine."""
import argparse,hashlib,json,os,statistics,subprocess
from pathlib import Path
from math_virtual import ROOT,measure
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',required=True);p.add_argument('--runs',type=int,default=7);p.add_argument('--threads',default='auto');p.add_argument('--source',default='egglog/tests/math-microbenchmark.egg');a=p.parse_args()
if a.runs<1:p.error('positive runs required')
if a.threads=='auto':os.environ.pop('RAYON_NUM_THREADS',None)
else:os.environ['RAYON_NUM_THREADS']=str(int(a.threads))
out=(ROOT/a.output).resolve();out.mkdir(parents=True,exist_ok=False);source=(ROOT/a.source).resolve();bindir=ROOT/'out/native-compare/bin'
variants=['pristine','after','pristine-mi','after-mi']
r={'source':str(source),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'harness_sha256':hashlib.sha256((ROOT/'experiments/native_compare/main.rs').read_bytes()).hexdigest(),'baseline_tag':subprocess.check_output(['git','rev-parse','egglog-baseline'],cwd=ROOT,text=True).strip(),'rustc':subprocess.check_output(['rustc','-Vv'],text=True),'threads':a.threads,'binary_sha256':{v:hashlib.sha256((bindir/v).read_bytes()).hexdigest() for v in variants},'runs':[],'notes':['Pristine source from immutable egglog-baseline; current engine has inactive tracing/virtualization extensions. No analysis, macros, profiling or graph scans in timed runs.','Both builds use identical external harness, dependency lock, release ThinLTO/codegen-units=1; syn/full is unified for host proc-macro compatibility without changing pristine source.','Variants ending in -mi use the same mimalloc allocator as the original CLI. Other variants use System.','Internal timer includes EGraph creation, parser and program execution; through_drop additionally includes engine/report destruction. Process measurements include startup, source read, and final JSON.']}
for i in range(a.runs):
 for v in variants[i%4:]+variants[:i%4]:
  x=measure([bindir/v,source],out/f'{v}-{i}');x.update(variant=v,repetition=i);r['runs'].append(x)
  (out/'report.json').write_text(json.dumps(r,indent=2)+'\n')
r['medians']={v:{'execution_seconds':statistics.median(x['result']['seconds'] for x in r['runs'] if x['variant']==v),'through_drop_seconds':statistics.median(x['result']['through_drop_seconds'] for x in r['runs'] if x['variant']==v),'wall_seconds':statistics.median(x['process_wall_seconds'] for x in r['runs'] if x['variant']==v),'cpu_seconds':statistics.median(x['user_seconds']+x['system_seconds'] for x in r['runs'] if x['variant']==v),'peak_rss_bytes':statistics.median(x['peak_rss_bytes'] for x in r['runs'] if x['variant']==v),'matches':sorted({x['result']['matches'] for x in r['runs'] if x['variant']==v})} for v in variants}
(out/'report.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r['medians'],indent=2))

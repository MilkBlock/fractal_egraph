"""One paired CPU measurement per workload; setup/export excluded from inference."""
import argparse,hashlib,json,sys,subprocess,platform
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT))
from eggfuse.engine.cpu import binary,execute
p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--cpu-bin',type=Path);p.add_argument('--cases',nargs='+',default=['fact'],choices=['fact','loopfact','sum-bits','raytrace']);p.add_argument('--instances',type=int,default=1024);p.add_argument('--stages',type=int,default=28);a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False);engine=a.cpu_bin.resolve() if a.cpu_bin else binary()
source='(datatype E (N i64) '+' '.join(f'(P{i} E)' for i in range(a.stages))+')\n'
source+='\n'.join(f'(ruleset s{i})' for i in range(a.stages))+'\n'
for i in range(a.stages):
    body='(= x (N n))' if i==0 else f'(= x (P{i-1} parent))'
    source+=f'(rule ({body}) ((P{i} x)) :ruleset s{i} :name "stage-{i}")\n'
source+=''.join(f'(N {i})\n' for i in range(a.instances))
source+='(run-schedule (saturate '+' '.join(f'(run s{i})' for i in range(a.stages))+'))\n'
chain=a.output/'chain.egg';chain.write_text(source);rows={}
rows['chain']=execute(chain,a.output/'chain',engine,True)
suite=json.loads((ROOT/'experiments/profile_suite/suite.json').read_text())
for name in a.cases:
    case=next(c for c in suite['cases'] if c['name']==name)
    data=b''.join((ROOT/'experiments/profile_suite'/part).read_bytes() for part in case['parts'])
    assert hashlib.sha256(data).hexdigest()==case['sha256']
    src=a.output/(name+'.egg');src.write_bytes(data)
    rows[name+'-pass3']=execute(src,a.output/(name+'-pass3'),engine,True)
    print(name,'full endpoint verified',flush=True)
for k,r in rows.items():print(k,'native',r['native']['inference_seconds'],'cell',r['candidate']['inference_seconds'],'speedup',r['speedup'],'inline',r['candidate']['cells']['inline_successor_invocations'],'skips',r['candidate']['cells']['whole_rule_queries_skipped'],flush=True)
(a.output/'results.json').write_text(json.dumps(rows,indent=2)+'\n')

summary={'_provenance':{
    'base_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
    'kernel_worktree_diff_sha256':hashlib.sha256(subprocess.check_output(['git','diff','HEAD','--','egglog'],cwd=ROOT)).hexdigest(),
    'platform':platform.platform(),'native_threads':1,
    'control':'same binary, local native egglog; not pristine upstream',
    'sampling':'one paired run per input; inference excludes setup and audit',
}}
for name,r in rows.items():
    c=r['candidate'];n=r['native'];stats=c['cells']
    summary[name]={
        'source_sha256':r['source_sha256'],'binary_sha256':r['binary_sha256'],
        'native_seconds':n['inference_seconds'],'cell_seconds':c['inference_seconds'],
        'ratio_native_over_cell':r['speedup'],
        'native_bootstrap_seconds':n['bootstrap_seconds'],
        'cell_bootstrap_seconds':c['bootstrap_seconds'],'cell_setup_seconds':c['setup_seconds'],
        'recipe_compile_seconds':r['recipe_compile_seconds'],
        'verification':r['verification'],
        'compiled_scopes':stats['compiled_scopes'],
        'inline_successor_invocations':stats['inline_successor_invocations'],
        'whole_rule_queries_skipped':stats['whole_rule_queries_skipped'],
        'native_effect_receipts':stats['native_effect_receipts'],
        'private_receipt_tables':stats['private_receipt_tables'],
        'rejected_scopes':stats['rejected_scopes'],
        'cell_credit':stats['inline_successor_invocations']>0,
    }
(a.output/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')

"""Generic Analyze recipes: actual native capture, standalone load and row reconstruction.

No GPU inference benchmark is performed. Existing output directories are never
silently overwritten; --read-only summarizes already verified results.
"""
import argparse,json,os,subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]

def summarize(out):
    result={}
    for folder in sorted(out.iterdir()):
        if not folder.is_dir() or not (folder/'cell-recipes.json').exists():continue
        lib=json.loads((folder/'cell-recipes.json').read_text());report=json.loads((folder/'analysis.json').read_text());check=json.loads((folder/'verification.json').read_text())
        registered=report['saturated_rule_composition']['cell_egraph']['registered_compositions']
        assert lib['counts'].get('ready',0)==registered,'a registered Cell lost its executable representation'
        assert check['failed']==0
        result[folder.name]={'jobs':lib['cell_count'],'registered':registered,'programs':lib['counts'].get('ready',0),'counts':lib['counts'],'verified_endpoints':check['checked'],
                             'environments':lib['environment_count'],'expression_nodes':lib['expression_nodes'],'instruction_nodes':lib['instruction_nodes'],'library_bytes':(folder/'cell-recipes.json').stat().st_size}
    if (out/'unbounded').exists():
        lib=json.loads((out/'unbounded/cell-recipes.json').read_text());assert lib['counts'].get('ready')==1
        assert all('completion' not in c for c in lib['cells'])
    return result

def main():
    p=argparse.ArgumentParser();p.add_argument('--analyzer',type=Path,default=ROOT/'target/debug/egg_layout');p.add_argument('--checker',type=Path,default=ROOT/'target/debug/examples/cell_recipe');p.add_argument('--output',type=Path,required=True);p.add_argument('--pass3-source',type=Path);p.add_argument('--read-only',action='store_true');a=p.parse_args()
    if not a.read_only:
        a.output.mkdir(parents=True,exist_ok=False);cases={n:HERE/(n+'.egg') for n in ['positive','mixed','unbounded']}
        if a.pass3_source:
            source=a.pass3_source.read_text();marker='(ruleset initialization)';assert marker in source
            prefix=source[:source.index(marker)];facts=(HERE/'pass3-inputs.eggfrag').read_text()
            for name,schedule in [('terms','(saturate (run terms) (saturate (run terms-helpers) (saturate (run terms-helpers-helpers))))'),('resolved','(saturate (run is-resolved))')]:
                f=a.output/(name+'.egg');f.write_text(prefix+facts+'(run-schedule '+schedule+')\n');cases[name]=f
        for name,source in cases.items():
            folder=a.output/name
            with (a.output/(name+'.log')).open('w') as log:
                subprocess.run([str(a.analyzer.resolve()),'analyze','--source',str(source.resolve()),'--output',str(folder.resolve())],cwd=ROOT,env=dict(os.environ,EGG_LAYOUT_GRAPHS='0'),stdout=log,stderr=subprocess.STDOUT,check=True)
                subprocess.run([str(a.checker.resolve()),str((folder/'cell-recipes.json').resolve()),str((folder/'verification.json').resolve()),'all'],cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,check=True)
    result=summarize(a.output);(a.output/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))

if __name__=='__main__':main()

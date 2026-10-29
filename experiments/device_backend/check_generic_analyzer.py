"""Analyze -> exported typed recipe -> fresh GPU process -> native oracle.

Oracle rows are stored separately and never provided to inference.
"""
import argparse,json,os,subprocess
from pathlib import Path
from run_generic_cells import run_library,run_source
from run import compare
from eggfuse.analyzer.native import publish_recipes
from eggfuse.ir.library import adapt,decode

def check(source,analyzer,build,out):
    out.mkdir(parents=True,exist_ok=False);analysis=out/'analysis'
    with (out/'analyze.log').open('w') as log:
        subprocess.run([str(analyzer.resolve()),'analyze','--source',str(source.resolve()),'--output',str(analysis.resolve())],
            env=dict(os.environ,EGG_LAYOUT_GRAPHS='0',EGG_LAYOUT_DEVICE_ORACLE='1'),stdout=log,stderr=subprocess.STDOUT,check=True)
    libpath=publish_recipes(analysis);lib=decode(json.loads(libpath.read_text()))
    finite=dict(lib,instances=[i for i in lib['instances'] if i['observed_completion']=='Saturated'])
    result=run_library(adapt(finite),build,out/'cells')
    if not result['complete']:raise AssertionError(result['rejected'])
    checks=[]
    for instance in finite['instances']:
        unit=instance['unit'];folder=out/'cells'/f'program-{instance["program"]}'/f'cell-{unit}'
        p=json.loads((folder/'program.json').read_text());s=json.loads((folder/'gpu-state.json').read_text())
        ref=json.loads((analysis/'saturated-rule-composition/cells'/f'CSUnit-{unit:06}'/'cell-device-reference.json').read_text())
        checks.append(dict(unit=unit,**compare(p,s['rows'],s['parents'],ref['checkpoint'],s['sets'])))
    source_result=run_source(source,libpath,build,out/'source',capacity=256,match_capacity=1024)
    report=dict(recipe_schema='eggfuse-recipe-library/v1',cells=checks,source=source_result['native_verification'],
                actual_initialized_cells=len(lib['instances']),finite_cells=len(finite['instances']),
                typed_programs=len(lib['programs']),
                excluded_unfinished=[i['unit'] for i in lib['instances'] if i['observed_completion']!='Saturated'])
    (out/'verification.json').write_text(json.dumps(report,indent=2)+'\n');return report

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--analyzer',type=Path,default=Path('target/debug/egg_layout'));p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    print(json.dumps(check(a.source,a.analyzer,a.build,a.output),indent=2))

"""The shared SK/Bake/marker pipeline's standalone Cell CPU target."""
import json,shutil
from pathlib import Path
from eggfuse.ir.cell_runtime import build,validate
from eggfuse.ir.recipe import validate as validate_recipe
from eggfuse.compiler.passes import digest

ROOT=Path(__file__).resolve().parents[3]

def emit(folder,core,recipe,marker,source):
    validate_recipe(json.loads(recipe.read_text()),json.loads(core.read_text()),'original_program')
    program=build(json.loads(core.read_text()));validate(program)
    (folder/'program.json').write_text(json.dumps(program))
    (folder/'multi-stage-program.json').write_text(json.dumps(program['multi_stage'],indent=2))
    for name,path in [('core.json',core),('recipe-program.json',recipe),('marker-program.json',marker),('source_sk_mk.egg',source)]:shutil.copyfile(path,folder/name)
    caps={'backend':'cell-cpu','storage':'shared recipe and parent projections; virtual fact references; mutable function ports',
          'native_runtime_linked':False,'requirements':program['requirements'],
          'multi_stage':{'routes':len(program['multi_stage']['routes']),'publication':'original Run occurrences','local_results':'producer recipe slots with current exhaustive-domain checks','residual':'all ambiguous or unknown boundary alternatives retained'},
          'limits':['no GPU execution','unproven inverse reads retain residual matching','diagnostic unit-cost extraction only','source frontend restricts post-initialization commands']}
    (folder/'capabilities.json').write_text(json.dumps(caps,indent=2))
    files=['program.json','multi-stage-program.json','core.json','recipe-program.json','marker-program.json','source_sk_mk.egg','capabilities.json']
    (folder/'manifest.json').write_text(json.dumps({'schema':'eggfuse-device-artifact/v1','backend':'cell-cpu','source_sha256':digest(source),'execution':{'native_runtime':False},'files':{n:digest(folder/n) for n in files}},indent=2))

def compile_target(pm,core,recipe,marker,source,stop_after,execute_runtime):
    implementations=[Path(__file__),ROOT/'eggfuse/ir/cell_runtime.py',ROOT/'eggfuse/ir/recipe.py',ROOT/'eggfuse/analyzer/multistage.py']
    outputs=['program.json','multi-stage-program.json','core.json','recipe-program.json','marker-program.json','source_sk_mk.egg','capabilities.json','manifest.json']
    def check(folder):validate(json.loads((folder/'program.json').read_text()))
    device=pm.run('device',{'core':core,'recipe':recipe,'marker':marker,'source':source},{'backend':'cell-cpu'},implementations,outputs,
        lambda f:emit(f,core,recipe,marker,source),check,preserves=['typed marked program and source control'])
    for name in outputs:pm.publish(device/name,'eggfuse_related_files/'+name)
    if stop_after=='device':return pm
    from eggfuse.engine.cell_runtime import run_artifact,binary
    engine=binary() if execute_runtime else None
    def verify(folder):
        result=run_artifact(device,folder/'cell',check=True,engine=engine) if execute_runtime else {'static_contract_validated':True,'executed':False}
        (folder/'verification.json').write_text(json.dumps(result,indent=2))
    files=implementations+list((ROOT/'engine/cell-runtime/src').glob('*.rs'))+[ROOT/'eggfuse/engine/cell_runtime.py',ROOT/'experiments/cell_runtime/audit.py']
    inputs={'program':device/'program.json'}
    if engine:inputs['engine']=engine
    result=pm.run('verify',inputs,{'backend':'cell-cpu','execute':execute_runtime},files,['verification.json'],verify,
        lambda f:json.loads((f/'verification.json').read_text()),preserves=['reported binding audit and fresh-process reproducibility'])
    pm.publish(result/'verification.json','verification.json');return pm

"""The shared SK/Bake/marker pipeline's standalone Cell CPU target."""
import json,shutil,os
from pathlib import Path
from eggfuse.ir.cell_runtime import build,validate
from eggfuse.ir.recipe import validate as validate_recipe
from eggfuse.compiler.passes import digest

ROOT=Path(__file__).resolve().parents[3]

def emit(folder,core,recipe,marker,source,compositions=None):
    validate_recipe(json.loads(recipe.read_text()),json.loads(core.read_text()),'original_program')
    program=build(json.loads(core.read_text()),json.loads(marker.read_text()),compositions=json.loads(compositions.read_text()) if compositions else None);validate(program)
    (folder/'program.json').write_text(json.dumps(program))
    (folder/'reference-views.json').write_text(json.dumps(program['reference_views'],indent=2))
    (folder/'multi-stage-program.json').write_text(json.dumps(program['multi_stage'],indent=2))
    (folder/'merge-contracts.json').write_text(json.dumps(program['merge_contracts'],indent=2))
    (folder/'local-congruence-program.json').write_text(json.dumps(program['congruence'],indent=2))
    for name,path in [('core.json',core),('recipe-program.json',recipe),('marker-program.json',marker),('source_sk_mk.egg',source)]:shutil.copyfile(path,folder/name)
    caps={'backend':'cell-cpu','storage':'shared recipe and parent projections; virtual fact references; mutable function ports',
          'native_runtime_linked':False,'requirements':program['requirements'],
          'multi_stage':{'routes':len(program['multi_stage']['routes']),'publication':'original Run occurrences','local_results':'producer recipe slots with current exhaustive-domain checks','residual':'all ambiguous or unknown boundary alternatives retained'},
          'reference_views':{'enable':'CELL_CPU_REFERENCE_VIEWS=1','marker_tables':len(program['reference_views']['marker_tables']),'layout':'shared typed slot projections; immutable FactRef providers','takeover':'exact whole-source containment at Run publication; mutable function owners retained'},
          'local_congruence':{'templates':len(program['congruence']['templates']), 'enable':'CELL_CPU_CONGRUENCE=1', 'proof':'conditional constructor congruence checked against typed recipe', 'fallback':'global repair; shortcut disabled with non-lattice function merges'},
          'limits':['no GPU execution','unproven inverse reads retain residual matching','diagnostic unit-cost extraction only','source frontend restricts post-initialization commands']}
    (folder/'capabilities.json').write_text(json.dumps(caps,indent=2))
    files=['program.json','reference-views.json','multi-stage-program.json','local-congruence-program.json','merge-contracts.json','core.json','recipe-program.json','marker-program.json','source_sk_mk.egg','capabilities.json']
    if compositions:
        shutil.copyfile(compositions,folder/'composition-program.json');files.append('composition-program.json')
        caps['compositions']={'programs':len(program['compositions']['programs']),'edges':len(program['compositions']['edges']),'enable':'CELL_CPU_COMPOSITIONS=1','default':False}
        (folder/'capabilities.json').write_text(json.dumps(caps,indent=2))
    (folder/'manifest.json').write_text(json.dumps({'schema':'eggfuse-device-artifact/v1','backend':'cell-cpu','source_sha256':digest(source),'execution':{'native_runtime':False},'files':{n:digest(folder/n) for n in files}},indent=2))

def compile_target(pm,core,recipe,marker,source,stop_after,execute_runtime,compositions=None):
    implementations=[Path(__file__),ROOT/'eggfuse/ir/cell_runtime.py',ROOT/'eggfuse/ir/recipe.py',ROOT/'eggfuse/analyzer/multistage.py',ROOT/'eggfuse/analyzer/congruence.py',ROOT/'eggfuse/analyzer/reference_views.py',ROOT/'eggfuse/compiler/marker_ir.py',ROOT/'eggfuse/ir/structural_key.py',ROOT/'engine/structural-key/abi.json']
    outputs=['program.json','reference-views.json','multi-stage-program.json','local-congruence-program.json','merge-contracts.json','core.json','recipe-program.json','marker-program.json','source_sk_mk.egg','capabilities.json','manifest.json']
    if compositions:
        outputs.append('composition-program.json');implementations.append(ROOT/'eggfuse/analyzer/compositions.py')
    def check(folder):validate(json.loads((folder/'program.json').read_text()))
    device=pm.run('device',{'core':core,'recipe':recipe,'marker':marker,'source':source,**({'compositions':compositions} if compositions else {})},{'backend':'cell-cpu'},implementations,outputs,
        lambda f:emit(f,core,recipe,marker,source,compositions),check,preserves=['typed marked program and source control'])
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
    runtime_options={k:v for k,v in os.environ.items() if k.startswith('CELL_CPU_')}
    result=pm.run('verify',inputs,{'backend':'cell-cpu','execute':execute_runtime,'runtime_options':runtime_options},files,['verification.json'],verify,
        lambda f:json.loads((f/'verification.json').read_text()),preserves=['reported binding audit and fresh-process reproducibility'])
    pm.publish(result/'verification.json','verification.json');return pm

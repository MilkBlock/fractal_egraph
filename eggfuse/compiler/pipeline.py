"""Single compiler entry; the marked .egg is the CPU/GPU semantic reference."""
import argparse,hashlib,json,os,shutil,subprocess,sys
from pathlib import Path
from .passes import PassManager,digest
from .marker_ir import build as build_markers,validate as validate_markers

ROOT=Path(__file__).resolve().parents[2]
BACKEND=ROOT/'experiments/device_backend'
sys.path.insert(0,str(BACKEND))

def command(args,log):
    with Path(log).open('w') as f:subprocess.run(list(map(str,args)),cwd=ROOT,env=dict(os.environ,EGG_LAYOUT_GRAPHS='0'),stdout=f,stderr=subprocess.STDOUT,check=True)

def native_core(frontend,source,folder):
    with (folder/'frontend.log').open('w') as error:
        proc=subprocess.run([str(frontend),str(source)],input='',text=True,stdout=subprocess.PIPE,stderr=error,check=True)
    return json.loads(proc.stdout.splitlines()[0])

def compile_pipeline(source,output,build,tools,solver_python,profile=None,stop_after='verify',run_gpu=True,capacity=4096,match_capacity=16384,backend='gpu',multi_stage_cell=False,bake_source=None):
    source=Path(source).resolve();build=Path(build).resolve();tools=Path(tools).resolve()
    if multi_stage_cell and backend!='gpu':raise ValueError('multi-stage Cell selects the regions runtime; do not combine it with --backend local')
    if multi_stage_cell:backend='regions'
    pm=PassManager(output,stop_after);source_file=pm.publish(source,'source.egg')
    base=[Path(__file__),ROOT/'eggfuse/compiler/passes.py']
    ast_binary=tools/'lhs_ast';key_binary=tools/'structural_key_source';frontend=tools/'eggfuse_frontend';analyzer=tools.parent/'egg_layout'
    needed=[ast_binary,key_binary]
    if stop_after!='sk':needed.append(analyzer)
    if stop_after not in ('sk','analyze') or bake_source is not None:needed.append(frontend)
    if stop_after=='verify' and run_gpu and backend=='gpu':needed.append(build/'worker')
    for binary in needed:
        if not binary.is_file():raise ValueError('missing built tool '+str(binary))
    def valid_json(folder,name):json.loads((folder/name).read_text())
    key_code=base+[ROOT/'tools/cell_spine'/n for n in ['structural_keys.py','key_demand.py','terms.py','pass3_markers.py']]
    def keys(folder):
        from tools.cell_spine.structural_keys import compile_source
        compile_source(source_file,folder/'result',metadata_binary=key_binary,ast_binary=ast_binary)
    sk=pm.run('structural_keys',{'source':source_file,'metadata_tool':key_binary,'ast_tool':ast_binary},
        {'policy':'source-declared only'},key_code,['result/source_sk.egg','result/report.json','result/policy.json'],keys,
        lambda f:valid_json(f/'result','report.json'),preserves=['unannotated choice policy'])/'result'
    sk_source=pm.publish(sk/'source_sk.egg','source_sk.egg')
    if stop_after=='sk':return pm
    evidence_source=sk_source
    if bake_source is not None:
        evidence_source=pm.publish(Path(bake_source).resolve(),'bake-source.egg')
        def check_environment(folder):
            from .multistage import validate_bake_environment
            target=native_core(frontend,sk_source,folder);sample=native_core(frontend,evidence_source,folder)
            report=validate_bake_environment(target,sample)
            (folder/'environment.json').write_text(json.dumps(report,indent=2))
        pm.run('bake_environment',{'source_sk':sk_source,'bake_source':evidence_source,'frontend':frontend},{},
            base+[ROOT/'eggfuse/compiler/multistage.py'],['environment.json'],check_environment,lambda f:valid_json(f,'environment.json'),preserves=['same typed training rule environment'])
    def analyze(folder):
        command([analyzer,'analyze','--source',evidence_source,'--output',folder/'result'],folder/'analyze.log')
        from eggfuse.analyzer.native import publish_recipes
        publish_recipes(folder/'result')
    analysis=pm.run('analyze',{'source_sk':evidence_source,'analyzer':analyzer},{'graphs':False},base+[ROOT/'eggfuse/analyzer/native.py',ROOT/'eggfuse/analyzer/recipes.py',ROOT/'eggfuse/analyzer/regions.py',ROOT/'eggfuse/analyzer/batches.py',ROOT/'eggfuse/ir/batches.py',ROOT/'eggfuse/ir/regions.py',ROOT/'eggfuse/ir/library.py',ROOT/'eggfuse/ir/recipe.py'],
        ['result/recipe-library.json','result/cell-recipes.json','result/cell-device-recipes.json','result/analysis.json','result/catalog/*.json','result/catalog/states/*.json','result/saturated-rule-composition/cells/*/observed-saturated-state.json'],
        analyze,
        lambda f:valid_json(f/'result','cell-recipes.json'),preserves=['source semantics'])/'result'
    # The catalogue is an explicit dependency: changing Bake invalidates only
    # consumers of its evidence, not the already completed SK pass.
    witness_files=sorted((analysis/'catalog/states').glob('*.json'))+sorted((analysis/'saturated-rule-composition/cells').glob('*/observed-saturated-state.json'))
    witness_index=pm.output/'analysis-index.json';witness_index.write_text(json.dumps({str(p.relative_to(analysis)):digest(p) for p in witness_files},sort_keys=True))
    if stop_after=='analyze':return pm
    marker_inputs={'source_sk':sk_source,'analysis':analysis/'cell-recipes.json','catalog':witness_index,'ast_tool':ast_binary,'solver_python':solver_python}
    venv=Path(solver_python).parent.parent/'pyvenv.cfg'
    if venv.exists():marker_inputs['solver_environment']=venv
    if profile:marker_inputs['profile']=Path(profile).resolve()
    def markers(folder):
        request=dict(source=str(sk_source),analysis=str(analysis),output=str(folder/'result'),ast_binary=str(ast_binary),profile=str(Path(profile).resolve()) if profile else None)
        (folder/'request.json').write_text(json.dumps(request))
        command([solver_python,'-m','eggfuse.compiler.select_markers',folder/'request.json'],folder/'markers.log')
    marked=pm.run('markers',marker_inputs,{'selection':'profile' if profile else 'explicit structural baseline'},
        base+[ROOT/'tools/cell_spine'/n for n in ['pass3_markers.py','solver.py','terms.py','takeover.py','representation.py','lhs_coverage.py']]+[ROOT/'eggfuse/compiler/select_markers.py'],
        ['result/published.egg','result/shared.egg','result/markers.json'],markers,lambda f:valid_json(f/'result','markers.json'),
        preserves=['original guards and effects','full correlated binding keys'])/'result'
    marked_source=pm.publish(marked/'published.egg','source_sk_mk.egg')
    def marker_ir(folder):
        core=native_core(frontend,marked_source,folder)
        ir=build_markers(json.loads((marked/'markers.json').read_text()),core,marked_source.read_bytes(),(analysis/'cell-recipes.json').read_bytes())
        (folder/'core.json').write_text(json.dumps(core));(folder/'marker-program.json').write_text(json.dumps(ir,indent=2))
    ir=pm.run('marker_ir',{'marked_source':marked_source,'markers':marked/'markers.json','analysis':analysis/'cell-recipes.json','frontend':frontend},{},
        base+[ROOT/'eggfuse/compiler/marker_ir.py'],['core.json','marker-program.json'],marker_ir,
        lambda f:validate_markers(json.loads((f/'marker-program.json').read_text()),json.loads((f/'core.json').read_text())),preserves=['typed marker contract'])
    marker_file=pm.publish(ir/'marker-program.json','marker-program.json')
    if stop_after=='markers':return pm
    def recipe_ir(folder):
        from eggfuse.ir.recipe import build
        value=build(json.loads((ir/'core.json').read_text()))
        (folder/'recipe-program.json').write_text(json.dumps(value,indent=2))
    def check_recipe(folder):
        from eggfuse.ir.recipe import validate
        validate(json.loads((folder/'recipe-program.json').read_text()),json.loads((ir/'core.json').read_text()),'original_program')
    recipe=pm.run('recipe_ir',{'core':ir/'core.json'},{'scope':'original_program'},
        base+[ROOT/'eggfuse/ir/recipe.py'],['recipe-program.json'],recipe_ir,check_recipe,preserves=['typed query/effect/control and binding correlation'])
    recipe_file=pm.publish(recipe/'recipe-program.json','recipe-program.json')
    if stop_after=='recipe':return pm
    region_file=None
    execution=dict(stage_contexts=False,joint_polyspine=False,block_merge=False,matrix_saturate=False)
    if multi_stage_cell:
        def regions(folder):
            from eggfuse.analyzer.regions import analyze
            marker_names=[x['name'] for x in json.loads(marker_file.read_text())['markers']]
            plan=analyze(json.loads(recipe_file.read_text()),marker_names)
            (folder/'region-program.json').write_text(json.dumps(plan,indent=2))
        def check_regions(folder):
            from eggfuse.analyzer.regions import verify
            verify(json.loads((folder/'region-program.json').read_text()),json.loads(recipe_file.read_text()))
        region=pm.run('multi_stage_cell',{'recipe_ir':recipe_file,'marker_ir':marker_file},
            {'mode':'generic correlated Spine regions; residual queries retained'},
            base+[ROOT/'eggfuse/analyzer/regions.py',ROOT/'eggfuse/analyzer/batches.py',ROOT/'eggfuse/ir/batches.py',ROOT/'eggfuse/ir/regions.py',ROOT/'eggfuse/ir/recipe.py'],
            ['region-program.json'],regions,check_regions,preserves=['full rule set, correlated witnesses and original stage control'])
        region_file=pm.publish(region/'region-program.json','region-program.json')
    def lower(folder):
        from .targets.metal.compiler import compile_program
        from eggfuse.ir.recipe import instantiate
        original=json.loads((ir/'core.json').read_text());recipe_value=json.loads(recipe_file.read_text())
        core=instantiate(recipe_value,original['initial'],'original_program');m=json.loads(marker_file.read_text())
        if backend in ('local','regions'):
            from .targets.metal.generic_cell import lower as local_lower
            program=local_lower(core,capacity,match_capacity,marker_program=m,region_program=json.loads(region_file.read_text()) if region_file else None)
        else:
            program=compile_program(core,capacity=capacity,match_capacity=match_capacity,marker_program=m,
                                    **execution)
        program['recipe_ir']=dict(schema=recipe_value['schema'],sha256=recipe_value['sha256'],scope=recipe_value['scope'])
        shutil.copyfile(recipe_file,folder/'recipe-program.json')
        shutil.copyfile(marker_file,folder/'marker-program.json')
        shutil.copyfile(marked_source,folder/'source_sk_mk.egg')
        if region_file:shutil.copyfile(region_file,folder/'region-program.json')
        from .capabilities import report as capability_report
        (folder/'capabilities.json').write_text(json.dumps(capability_report(recipe_value,program,backend),indent=2))
        (folder/'program.json').write_text(json.dumps(program));(folder/'kernel.metal').write_text(program['shader'])
        shutil.copyfile(ir/'core.json',folder/'core.json')
        (folder/'manifest.json').write_text(json.dumps(dict(schema='eggfuse-device-artifact/v1',backend=backend,execution=execution,source_sha256=digest(marked_source),marker_sha256=m['sha256'],
            files={n:digest(folder/n) for n in ['core.json','program.json','kernel.metal','recipe-program.json','capabilities.json','marker-program.json','source_sk_mk.egg']+(['region-program.json'] if region_file else [])}),indent=2))
    lowering_files=[ROOT/'eggfuse/compiler/targets/metal'/(name+'.py') for name in ['compiler','batch_rhs','matrix_sat','matrix_comparison','joint_polyspine','query_contexts','context_ridge','stage_cell','cell_program','cell_updates','cell_spines','scheduled_matrix','generic_cell','regions','batches','resident_indexes']]
    device=pm.run('device',{'core':ir/'core.json','marker_ir':marker_file,'recipe_ir':recipe_file,**({'regions':region_file} if region_file else {})},dict(backend=backend,capacity=capacity,match_capacity=match_capacity,**execution),
        base+lowering_files+list((ROOT/'engine/metal').glob('*.metal'))+list((ROOT/'eggfuse/compiler/recipes').glob('*.py'))+[ROOT/'eggfuse/compiler/marker_ir.py',ROOT/'eggfuse/compiler/multistage.py',ROOT/'eggfuse/ir/recipe.py',ROOT/'eggfuse/ir/interface.py',ROOT/'eggfuse/compiler/capabilities.py',ROOT/'eggfuse/ir/regions.py',ROOT/'eggfuse/ir/batches.py'],['program.json','kernel.metal','core.json','manifest.json','recipe-program.json','capabilities.json','marker-program.json','source_sk_mk.egg']+(['region-program.json'] if region_file else []),lower,
        lambda f:valid_json(f,'program.json'),preserves=['marked program semantics'])
    for filename in ['program.json','kernel.metal','core.json','manifest.json','recipe-program.json','capabilities.json','marker-program.json','source_sk_mk.egg']+(['region-program.json'] if region_file else []):pm.publish(device/filename,'eggfuse_related_files/'+filename)
    if stop_after=='device':return pm
    if backend=='regions':
        def verify_regions_runtime(folder):
            from eggfuse.engine.regions import run_artifact
            runtime=folder/'runtime';runtime.mkdir();os.symlink(frontend,runtime/'frontend')
            spec=json.loads((build/'build.json').read_text());spec.update(frontend=str(frontend),frontend_sha256=digest(frontend));(runtime/'build.json').write_text(json.dumps(spec))
            if run_gpu:
                result=run_artifact(device,runtime,folder/'gpu',check=True)
                checked=dict(backend=backend,verification=result['verification'],multi_stage_cell=result['region_execution'],gpu_inference_seconds=result['inference_seconds'])
            else:checked=dict(backend=backend,static_contract_validated=True,gpu_checked=False)
            (folder/'verification.json').write_text(json.dumps(checked,indent=2))
        checked=pm.run('verify',{'program':device/'program.json','frontend':frontend,'source':marked_source},
            {'gpu':run_gpu,'contract':'soundness checks and GPU L1; no native whole-graph equivalence gate'},
            base+[ROOT/'eggfuse/engine/regions.py',ROOT/'eggfuse/ir/batches.py',ROOT/'eggfuse/engine/artifacts.py',ROOT/'engine/metal/generic_cell.swift',ROOT/'engine/metal/batched_cell.metal',BACKEND/'run_generic_cells.py'],
            ['verification.json'],verify_regions_runtime,lambda f:valid_json(f,'verification.json'),preserves=['checked derivations','repeatable GPU rounds'])
        pm.publish(checked/'verification.json','verification.json');return pm
    references=[]
    for name,reference in [('sk_native',sk_source),('marked_native',marked_source)]:
        references.append(pm.run(name,{'source':reference,'frontend':frontend},{'schedule':'original source commands'},base,
            ['result/native-state.json','result/report.json'],
            lambda f,reference=reference:command([frontend,reference,'--native-full','--export-to',f/'result'],f/'native.log'),
            lambda f:valid_json(f/'result','native-state.json'),preserves=['independent native oracle'])/'result')
    def verify(folder):
        from run import compare,run
        from .verify import compare_source
        result=compare_source(json.loads((references[0]/'native-state.json').read_text()),json.loads((references[1]/'native-state.json').read_text()),json.loads(marker_file.read_text()))
        if run_gpu:
            runtime=folder/'runtime';runtime.mkdir();os.symlink(frontend,runtime/'frontend');os.symlink(build/'worker',runtime/'worker')
            spec=json.loads((build/'build.json').read_text());spec.update(frontend=str(frontend),frontend_sha256=digest(frontend));(runtime/'build.json').write_text(json.dumps(spec))
            if backend in ('local','regions'):
                from run_generic_cells import run_library
                c=json.loads((ir/'core.json').read_text());initial=c.pop('initial')
                library=dict(schema='analyzer-device-cells/v1',programs=[c],instances=[dict(unit=0,program=0,initial=initial)])
                gpu=run_library(library,runtime,folder/'gpu',capacity,match_capacity,marker_program=json.loads(marker_file.read_text()),compiled_program=device/'program.json')
                if not gpu['complete']:raise ValueError('local device program did not complete')
                state_folder=folder/'gpu/program-0/cell-0';seconds=gpu['programs'][0]['inference_seconds']
            else:
                gpu=run(marked_source,runtime,folder/'gpu',check_every=0,capacity=capacity,match_capacity=match_capacity,save_final_state=True,marker_program=marker_file,compiled_program=device/'program.json',**execution)
                state_folder=folder/'gpu';seconds=gpu['inference_seconds']
            program=json.loads((state_folder/'program.json').read_text());state=json.loads((state_folder/'gpu-state.json').read_text())
            checks=set(program['assertion_tables']);rows=[r for r in state['rows'] if program['tables'][r[0]]['name'] not in checks]
            if multi_stage_cell:
                from eggfuse.engine.regions import execution_summary
                result['multi_stage_cell']=execution_summary(gpu)
            result['gpu_inference_seconds']=seconds;result['backend']=backend;result['device_marker_contract']=program['marker_program']
            try:
                result['gpu']=compare(program,rows,state['parents'],json.loads((references[1]/'native-state.json').read_text()),state['sets'])
            except (ValueError,AssertionError) as error:
                result['gpu']={'exact_rows_values_equivalence_visibility':False,'error':str(error)}
                (folder/'verification.json').write_text(json.dumps(result,indent=2))
                raise
        (folder/'verification.json').write_text(json.dumps(result,indent=2))
    verify_inputs={'sk_reference':references[0]/'native-state.json','mk_reference':references[1]/'native-state.json','marked_source':marked_source,'marker_ir':marker_file,'frontend':frontend,'device':device/'program.json'}
    if run_gpu:verify_inputs['worker']=build/'worker'
    verification=pm.run('verify',verify_inputs,
        {'gpu':run_gpu,'backend':backend,'shared_run_encoder':os.environ.get('EGGFUSE_SHARED_ENCODER')=='1','joint_shared_encoder':os.environ.get('EGGFUSE_JOINT_SHARED_ENCODER')=='1','joint_fuse_prepare':os.environ.get('EGGFUSE_JOINT_FUSE_PREPARE')=='1','joint_local_sort':os.environ.get('EGGFUSE_JOINT_LOCAL_SORT')=='1','compare':'complete original tables modulo canonical identity'},
        base+[ROOT/'eggfuse/compiler/verify.py',ROOT/'eggfuse/compiler/multistage.py',ROOT/'eggfuse/ir/recipe.py',ROOT/'eggfuse/ir/library.py',ROOT/'eggfuse/engine/artifacts.py',ROOT/'eggfuse/engine/regions.py',ROOT/'eggfuse/analyzer/regions.py',BACKEND/'run.py',BACKEND/'verify_native_source.py',BACKEND/'run_generic_cells.py',ROOT/'engine/metal/generic_cell.swift'],['verification.json'],verify,
        lambda f:valid_json(f,'verification.json'),preserves=['verified observable result'])
    pm.publish(verification/'verification.json','verification.json');return pm

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--build',type=Path,required=True)
    p.add_argument('--tools',type=Path,default=ROOT/'target/debug/examples');p.add_argument('--solver-python',type=Path,default=Path(sys.executable));p.add_argument('--profile',type=Path)
    p.add_argument('--bake-source',type=Path,help='Optional small training input; typed declarations/rules must match the execution environment');p.add_argument('--multi-stage-cell',action='store_true',help='Compile witnessed multi-stage batches; GPU context matrices, deterministic block publication and soundness/L1 checks');p.add_argument('--stop-after',choices=['sk','analyze','markers','recipe','device','verify'],default='verify');p.add_argument('--cpu-only',action='store_true');p.add_argument('--backend',choices=['gpu','local'],default='gpu');p.add_argument('--capacity',type=int,default=4096);p.add_argument('--match-capacity',type=int,default=16384);a=p.parse_args()
    pm=compile_pipeline(a.source,a.output,a.build,a.tools,a.solver_python.absolute(),a.profile,a.stop_after,not a.cpu_only,a.capacity,a.match_capacity,a.backend,a.multi_stage_cell,a.bake_source)
    print(json.dumps(dict(output=str(pm.output),passes=[dict(name=r['name'],cached=r['cached']) for r in pm.receipts],complete=a.stop_after=='verify',gpu_checked=a.stop_after=='verify' and not a.cpu_only),indent=2))

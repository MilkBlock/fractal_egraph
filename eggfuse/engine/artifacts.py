"""Load-time checks; the engine consumes a contract, not analyzer heuristics."""
import json
from eggfuse.ir.recipe import validate,require

def validate_recipe_artifact(folder,manifest,program,core):
    descriptor=program.get('recipe_ir')
    if 'recipe-program.json' not in manifest['files']:
        require(descriptor is None,'compiled recipe descriptor has no artifact')
        return None # legacy artifact, not silently upgraded
    recipe=json.loads((folder/'recipe-program.json').read_text())
    validate(recipe,core,scope='original_program')
    require(descriptor==dict(schema=recipe['schema'],sha256=recipe['sha256'],scope=recipe['scope']), 'compiled Recipe IR descriptor differs')
    if program.get('region_program') is not None:
        from eggfuse.ir.regions import validate_plan as verify
        require('region-program.json' in manifest['files'],'region descriptor has no artifact')
        regions=json.loads((folder/'region-program.json').read_text())
        verify(regions,recipe)
        require(regions==program['region_program'],'compiled regions differ from artifact')
        if program.get('resident_schedules') is not None:
            from eggfuse.compiler.targets.metal.resident_schedules import description
            expected,_,_=description(core,program,regions)
            require(program['resident_schedules']==expected,'private schedule certificate differs from typed source')
        if regions.get('batch_program') is not None:
            batch=regions['batch_program'];layout=program.get('batch_layout',{})
            width=max([len(t['slots']) for t in batch['templates']]+[1]);steps=max([len(t['steps']) for t in batch['templates']]+[1])
            require((layout.get('schema'),layout.get('stats_words')) in [('eggfuse-batch-layout/v3',24),('eggfuse-batch-layout/v4',28),('eggfuse-batch-layout/v5',34),('eggfuse-batch-layout/v6',40)],'batch receipt layout differs')
            require(layout.get('batch_program_sha256')==batch['sha256'],'batch layout contract differs')
            require(layout.get('width')==width and layout.get('steps')==steps and layout.get('stride')==2*width+steps+1 and layout.get('tile')==128,'batch physical layout differs')
            require(program.get('generic_cell',{}).get('transport_version')==7,'recompile batch artifact for current worker ABI')

    return recipe

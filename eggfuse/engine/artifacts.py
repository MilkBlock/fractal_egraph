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
    return recipe

"""Native egglog target for shared multi-stage recipes; no Metal lowering."""
import copy
from eggfuse.ir.recipe import build,validate
from eggfuse.analyzer.regions import from_core,verify

def lower(core,regions=None):
    recipe=build(core);validate(recipe)
    regions=from_core(core) if regions is None else verify(regions,recipe)
    return dict(schema='eggfuse-cpu-cells/v1',core=copy.deepcopy(core),recipe=recipe,regions=regions,
        contract='native storage/matcher/actions; independently checked positive saturation cones; original Run/Repeat fallback')

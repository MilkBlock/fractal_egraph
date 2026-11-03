"""Adapt native analyzer exports without upgrading their proof authority."""
import copy
from eggfuse.ir.recipe import build,validate,instantiate,digest,require

SCHEMA='eggfuse-recipe-library/v1'

def adapt(library):
    require(library.get('schema')=='analyzer-device-cells/v1','expected native analyzer device library')
    for i in library['instances']:
        require(i.get('live_substitution_authorized',False) is False,'isolated analyzer instance cannot authorize global substitution')
    value={k:copy.deepcopy(v) for k,v in library.items() if k not in ['schema','programs','source_program']}
    value.update(schema=SCHEMA,programs=[build(p,'isolated_cell') for p in library['programs']])
    source=library.get('source_program')
    if source is not None:
        value['source_program']={k:copy.deepcopy(v) for k,v in source.items() if k!='core'}
        if source.get('status')=='ready':
            value['source_program'].update(recipe=build(source['core'],'original_program'),initial=copy.deepcopy(source['core']['initial']))
    return {**value,'sha256':digest(value)}

def decode(library):
    require(library.get('schema')==SCHEMA,'unsupported recipe library')
    require(library.get('sha256')==digest({k:v for k,v in library.items() if k!='sha256'}),'recipe library checksum differs')
    value={k:copy.deepcopy(v) for k,v in library.items() if k not in ['schema','sha256','programs','source_program']}
    value.update(schema='analyzer-device-cells/v1',programs=[validate(p,scope='isolated_cell') for p in library['programs']])
    source=library.get('source_program')
    if source is not None:
        value['source_program']={k:copy.deepcopy(v) for k,v in source.items() if k not in ['recipe','initial']}
        if source.get('status')=='ready':value['source_program']['core']=instantiate(source['recipe'],source['initial'],'original_program')
    require(adapt(value)==library,'recipe library contract differs')
    return value

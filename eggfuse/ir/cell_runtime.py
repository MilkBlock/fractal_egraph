"""Cell storage contract. Rules provide shared wiring, not a terminal answer."""
import copy
from .structural_key import merge_contracts,validate_core
from .recipe import build as recipe_build, validate as recipe_validate, digest, require

SCHEMA = 'eggfuse-cell-runtime/v1'

def requirements(core):
    primitives={};effects=set();control=set()
    def walk(x):
        if isinstance(x,list):
            for v in x:walk(v)
        elif isinstance(x,dict):
            if all(k in x for k in ('primitive','inputs','output')):
                key=(x['primitive'],tuple(x['inputs']),x['output']);primitives[key]=primitives.get(key,0)+1
            if 'op' in x:effects.add(x['op'])
            for k in ('run','repeat','saturate','sequence','until_run','assert'):
                if k in x:control.add(k)
            for v in x.values():walk(v)
    walk(core)
    return dict(primitives=[dict(name=k[0],inputs=list(k[1]),output=k[2],occurrences=n) for k,n in sorted(primitives.items())],
                effects=sorted(effects),control=sorted(control),
                sorts=sorted({s['name'] for t in core['tables'] for s in t['schema']}))

def build(core):
    validate_core(core)
    from eggfuse.analyzer.multistage import build as multi_stage_build
    from eggfuse.analyzer.congruence import build as congruence_build
    recipe = recipe_build(core)
    recipe_validate(recipe, core)
    templates = []
    for rid, rule in enumerate(recipe['rules']):
        facts = [i for i,e in enumerate(rule['effects']) if e['kind'] in ('construct','set')]
        reads = [a for a in rule['query'] if 'table' in a['call']]
        templates.append(dict(rule=rid, fact_operations=facts,
            direct_entry=len(reads)==1, entry_table=reads[0]['call']['table'] if len(reads)==1 else None,
            primitive_guards=[i for i,a in enumerate(rule['query']) if 'primitive' in a['call']]))
    payload = dict(schema=SCHEMA, merge_contracts=merge_contracts(core), recipe=recipe, multi_stage=multi_stage_build(recipe), congruence=congruence_build(recipe), templates=templates,requirements=requirements(core),
        initial=copy.deepcopy(core.get('initial',[])),
        contract=dict(storage='shared effect recipes plus instance slots and fact-position masks',
                      native_runtime_dependency=False, native_free_join=False,
                      completeness='exact residual virtual-fact matching; no learned exclusion'),
        source_core=copy.deepcopy(core))
    return {**payload,'sha256':digest(payload)}

def validate(program):
    require(program.get('schema')==SCHEMA,'unsupported Cell runtime schema')
    require(program.get('sha256')==digest({k:v for k,v in program.items() if k!='sha256'}),'Cell artifact checksum differs')
    core=program['source_core']
    recipe_validate(program['recipe'],core,scope='original_program')
    expected=build(core)
    if 'multi_stage' not in program:  # Existing v1 controls remain valid, without optimization.
        expected.pop('multi_stage')
    if 'congruence' not in program: expected.pop('congruence')
    if 'merge_contracts' not in program: expected.pop('merge_contracts')
    expected['sha256']=digest({k:v for k,v in expected.items() if k!='sha256'})
    require(expected==program,'Cell wiring, entry, initial state or contract differs from typed source')
    return core

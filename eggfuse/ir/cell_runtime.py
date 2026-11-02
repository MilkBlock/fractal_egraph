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

def build(core, marker=None, reference_tables=(), compositions=None):
    validate_core(core)
    if marker is not None:
        from eggfuse.compiler.marker_ir import validate as validate_marker
        validate_marker(marker,core)
    from eggfuse.analyzer.multistage import build as multi_stage_build
    from eggfuse.analyzer.congruence import build as congruence_build
    from eggfuse.analyzer.merge_programs import build as merge_programs_build
    from eggfuse.analyzer.known_results import build as known_results_build
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
        merge_programs=merge_programs_build(recipe),
        known_results=known_results_build(recipe),
        initial=copy.deepcopy(core.get('initial',[])),
        contract=dict(storage='shared effect recipes plus instance slots and fact-position masks',
                      native_runtime_dependency=False, native_free_join=False,
                      completeness='exact residual virtual-fact matching; no learned exclusion'),
        source_core=copy.deepcopy(core))
    from eggfuse.analyzer.reference_views import build as views_build
    payload['reference_views']=views_build(recipe,[m['name'] for m in marker['markers']] if marker else reference_tables)
    if compositions is not None:
        from eggfuse.analyzer.compositions import validate as check_compositions
        payload['compositions']=check_compositions(compositions,recipe,payload['reference_views']['marker_tables'])
    if marker is not None:payload['marker_program_sha256']=marker['sha256']
    return {**payload,'sha256':digest(payload)}

def validate(program):
    require(program.get('schema')==SCHEMA,'unsupported Cell runtime schema')
    require(program.get('sha256')==digest({k:v for k,v in program.items() if k!='sha256'}),'Cell artifact checksum differs')
    core=program['source_core']
    recipe_validate(program['recipe'],core,scope='original_program')
    expected=build(core,reference_tables=program.get('reference_views',{}).get('marker_tables',[]),compositions=program.get('compositions'))
    if 'marker_program_sha256' in program:
        checksum=program['marker_program_sha256']
        require(isinstance(checksum,str) and len(checksum)==64 and all(c in '0123456789abcdef' for c in checksum),'marker checksum')
        expected['marker_program_sha256']=checksum
    if 'reference_views' not in program:expected.pop('reference_views')
    if 'multi_stage' not in program:  # Existing v1 controls remain valid, without optimization.
        expected.pop('multi_stage')
    elif 'union_entries' not in program['multi_stage']:
        expected['multi_stage'].pop('union_entries',None)  # Old artifact; new union-graft mode requires re-emission.
    if 'congruence' not in program: expected.pop('congruence')
    if 'merge_contracts' not in program: expected.pop('merge_contracts')
    if 'merge_programs' not in program: expected.pop('merge_programs')
    if 'known_results' not in program: expected.pop('known_results')
    elif 'static_aliases' not in program['known_results']:expected['known_results'].pop('static_aliases',None)
    expected['sha256']=digest({k:v for k,v in expected.items() if k!='sha256'})
    require(expected==program,'Cell wiring, entry, initial state or contract differs from typed source')
    return core

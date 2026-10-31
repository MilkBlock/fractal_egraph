"""Native egglog targets sharing typed recipes and symbolic composition."""
import copy
import json
from eggfuse.ir.recipe import build,validate
from eggfuse.analyzer.regions import from_core,verify


def preparation_program(recipe,regions):
    from eggfuse.analyzer.batches import analyze
    # Borrow symbolic wiring discovery, not saturation permission. The actual
    # source control remains unchanged and native matches gate every stage.
    discovery=copy.deepcopy(recipe)
    discovery['control']=[{'saturate':{'sequence':[{'run':s['name']} for s in recipe['rulesets']]}}]
    templates=analyze(discovery,regions['rules'])['templates']
    tables={t['name']:t for t in recipe['tables']}
    for t in templates:
        aliases={};ports={}
        def resolve(v):
            while 'slot' in v and v['slot'] in aliases:v=aliases[v['slot']]
            return copy.deepcopy(v)
        for step in t['steps']:
            rule=recipe['rules'][step['recipe_rule']];mapping=step['mapping']
            def val(v):return resolve(mapping[v['slot']]) if 'slot' in v else copy.deepcopy(v)
            def key(table,args):return table,json.dumps(args,sort_keys=True)
            # A function's published result is a reply port, not the value
            # proposed by a prior set (especially for Eq-valued custom merges).
            for atom in rule['query']:
                table=atom['call'].get('table')
                if table is None or tables[table]['kind']=='constructor':continue
                result=atom['args'][-1]
                port=ports.get(key(table,[val(v) for v in atom['args'][:-1]]))
                if port is not None and 'slot' in result:mapping[result['slot']]=copy.deepcopy(port)
            receipts={r['effect']:r for r in step['receipts']}
            for i,e in enumerate(rule['effects']):
                if e['kind']=='copy':aliases[mapping[e['dst']]['slot']]=val(e['arg'])
                if e['kind']=='set' and tables[e['call']['table']]['kind']!='constructor':
                    ports[key(e['call']['table'],[val(v) for v in e['args']])]=copy.deepcopy(receipts[i]['actual'])
        t.pop('scope',None)
    return dict(schema='eggfuse-cpu-preparation/v1',contract='preparation only; original native match and schedule authorize every effect',source_control=copy.deepcopy(recipe['control']),templates=templates)


def lower(core,regions=None,*,staged=True):
    recipe=build(core);validate(recipe)
    regions=from_core(core) if regions is None else verify(regions,recipe)
    return dict(staged_preparation=preparation_program(recipe,regions) if staged else None,schema='eggfuse-cpu-cells/v1',core=copy.deepcopy(core),recipe=recipe,regions=regions,
        contract='native storage/matcher/actions; eager certified cones or exact stage-gated local patches')

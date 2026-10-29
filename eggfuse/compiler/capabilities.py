"""Separate representable recipes, target lowering, and available fast scopes."""
def report(recipe,device,backend):
    regions=[]
    joint=device.get('joint_polyspine',{}).get('program')
    if joint:regions.append(dict(kind='joint',rulesets=joint['sets'],schedule=joint['schedule']))
    for p in device.get('matrix_saturate',{}).get('programs',[]):
        if not p['name'].startswith('__scheduled_matrix_'):regions.append(dict(kind='predicate_matrix',rulesets=[p['name']],requires='original single-ruleset Saturate scope'))
    for p in device.get('comparison_saturate',{}).get('programs',[]):
        regions.append(dict(kind='comparison_matrix',rulesets=[p['name']],requires='original certified comparison Saturate scope'))
    for p in (device.get('scheduled_matrix') or {}).get('programs',[]):regions.append(dict(kind='scheduled_matrix',rulesets=p['rulesets'],schedule=p['schedule']))
    plan=device.get('region_program')
    if plan:regions.append(dict(kind='generic_spine_regions',rulesets=[s['name'] for s in plan['rulesets']],schedule=plan['control'],cached_nodes=len(plan['nodes']),residual_queries=True))
    rows=[]
    for rs in recipe['rulesets']:
        for ordinal,rid in enumerate(rs['rule_ids']):
            r=recipe['rules'][rid]
            rows.append(dict(ruleset=rs['name'],ordinal=ordinal,recipe_rule=rid,name=r['metadata']['name'],
                expressed=True,verified=True,generic_lowered=True,
                available_fast_regions=[i for i,region in enumerate(regions) if rs['name'] in region['rulesets']],
                executed=None,query_atoms=len(r['query']),operations=r['footprint']['operations']))
    return dict(schema='eggfuse-capabilities/v1',recipe_sha256=recipe['sha256'],backend=backend,
                scope='compile-time availability; actual execution and effect counters belong to the runtime report',
                event_routes=dict(tables=len(recipe['event_routes']),runtime_subscriptions_installed=bool(plan),granularity='canonical table mutation versions' if plan else None),
                fast_regions=regions,rules=rows)

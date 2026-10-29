"""Admission and execution receipts for the existing exact stage Cell backend."""
def validate_compilation(program):
    joint=program.get('joint_polyspine',{})
    cell=program.get('cell_program',{})
    if not (joint.get('enabled') and joint.get('program') and program.get('stage_cell') and program.get('block_merge')):
        raise ValueError('multi-stage Cell was requested but joint stage lowering was not admitted')
    if not (cell.get('spine_program',{}).get('enabled') and cell.get('update_program')):
        raise ValueError('multi-stage Cell requires executable boundary and update recipes')
    target=joint['program']['schedule']
    def normalize_schedule(s):
        if 'sequence' in s:
            xs=list(map(normalize_schedule,s['sequence']))
            return xs[0] if len(xs)==1 else {'sequence':xs}
        if 'saturate' in s:return {'saturate':normalize_schedule(s['saturate'])}
        if 'repeat' in s:return {'repeat':s['repeat'],'body':normalize_schedule(s['body'])}
        return s
    def contains(s):
        if normalize_schedule(s)==target:return True
        return any(contains(x) for x in s.get('sequence',[])) or any(contains(s[k]) for k in ['body','saturate'] if k in s)
    if not any(contains(s) for s in program['schedule']):
        raise ValueError('multi-stage Cell has no eligible scope in the emitted schedule')


def execution_summary(report):
    calls=report['joint_polyspine']['calls'];accepted=[c for c in calls if c['accepted']]
    stages=report.get('stage_contexts',{})
    if not report['complete_schedule'] or not accepted or not stages.get('patches'):
        raise ValueError('requested multi-stage Cell did not execute a complete schedule with stage patches')
    total=lambda group,key:sum(c.get(group,{}).get(key,0) for c in accepted)
    return dict(scope='certified terms / terms-helpers / terms-helpers-helpers; other stages retain their original execution',
        accepted_scopes=len(accepted),rejected_scopes=len(calls)-len(accepted),stage_patches=stages['patches'],
        private_phases=sum(c['private_phases'] for c in accepted),
        boundary_recipe_actions=total('spine_program','actions'),boundary_fallback_actions=total('spine_program','fallback_actions'),
        update_recipe_actions=total('cell_updates','actions'),existing_result_updates=total('cell_updates','existing_result_actions'),
        guard_pairs_eliminated=total('cell_updates','guard_pairs_eliminated'),
        joint_gpu_seconds=sum(c['gpu_seconds'] for c in calls),joint_control_seconds=sum(c['control_seconds'] for c in calls),
        global_publications=sum(c['global_publications'] for c in accepted),
        counters_are='executed recipe actions; not necessarily committed mutations')


def validate_runtime_options(manifest,program,actual,executable):
    # The host chooses stage opcodes while the shader fixes buffer layouts.
    # Loading a kernel without the same host options must not silently disable it.
    expected=manifest.get('execution',dict(stage_contexts=False,joint_polyspine=False,block_merge=False,matrix_saturate=False))
    if actual!=expected:raise ValueError('compiled artifact execution options differ from runtime')
    if program['cell_program'].get('artifact_sha256')!=(executable['sha256'] if executable else None):
        raise ValueError('compiled executable Cell artifact differs from runtime')


def validate_bake_environment(target,sample):
    """Training may omit bootstrap initialization, never invent rule semantics."""
    tables={t['name']:t for t in target['tables'] if not t.get('internal')}
    for t in sample['tables']:
        if not t.get('internal') and t not in tables.values():
            raise ValueError('Bake declaration/merge differs: '+t['name'])
    rules={r['name']:r['rules'] for r in target['rulesets']}
    for r in sample['rulesets']:
        if r['rules'] and rules.get(r['name'])!=r['rules']:
            raise ValueError('Bake rules differ: '+r['name'])
    return dict(same_typed_training_environment=True,
        observed_rulesets=[r['name'] for r in sample['rulesets'] if r['rules']],
        scope='training input/schedule only; full execution input and schedule remain unchanged')

"""Execute a verified analyzer region artifact. No terms-specific runtime path."""
import json
import sys
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]


def execution_summary(report):
    instances=[s for p in report['programs'] for s in p['instances']]
    batch=[sum((s.get('batch_statistics') or [0]*24)[i] for s in instances) for i in range(24)]
    stats=[sum(s.get('region_statistics',[0]*16)[i] for s in instances) for i in range(16)]
    matched=sum(h[2] for s in instances for h in s['history'])
    private=sum(a.get('private_schedule_binding_outputs',0) for p in report['programs'] for a in p['attempts'])
    return dict(direct_single_step_invocations=sum(s['batch_statistics'][43] for s in instances if len(s.get('batch_statistics') or [])>43),
        witnessed_constructor_reads=sum(s['batch_statistics'][40] for s in instances if len(s.get('batch_statistics') or [])>41),
        witnessed_constructor_sets=sum(s['batch_statistics'][41] for s in instances if len(s.get('batch_statistics') or [])>41),
        seed_rows_examined=sum(s['batch_statistics'][36] for s in instances if len(s.get('batch_statistics') or [])>38),
        active_seed_rows=sum(s['batch_statistics'][37] for s in instances if len(s.get('batch_statistics') or [])>38),
        grounded_seed_jobs=sum(s['batch_statistics'][38] for s in instances if len(s.get('batch_statistics') or [])>38),
        proved_empty_cell_seeds=sum(s['batch_statistics'][34] for s in instances if len(s.get('batch_statistics') or [])>35),
        cell_noop_checks=sum(s['batch_statistics'][35] for s in instances if len(s.get('batch_statistics') or [])>35),
        parallel_constructor_requests=sum(s['batch_statistics'][30] for s in instances if len(s.get('batch_statistics') or [])>33),
        parallel_constructor_rows=sum(s['batch_statistics'][31] for s in instances if len(s.get('batch_statistics') or [])>33),
        constructor_batches=sum(s['batch_statistics'][32] for s in instances if len(s.get('batch_statistics') or [])>33),
        constructor_batch_fallbacks=sum(s['batch_statistics'][33] for s in instances if len(s.get('batch_statistics') or [])>33),
        proved_empty_rhs=sum((s.get('batch_statistics') or [])[28] for s in instances if len(s.get('batch_statistics') or [])>29),
        parallel_noop_checks=sum((s.get('batch_statistics') or [])[29] for s in instances if len(s.get('batch_statistics') or [])>29),
        matcher_binding_outputs=matched-private,private_schedule_binding_outputs=private,executed_rule_invocations=matched+batch[2],match_count_scope='exact matcher outputs, private schedule invocations and accepted inline invocations are counted separately; none is committed mutations',batch_tiles=batch[0],batch_seed_invocations=batch[1],validated_successor_invocations=batch[2],rejected_successor_invocations=batch[3],batch_effect_requests=batch[4],residual_invocations=batch[5],max_tile_rows=max([(s.get('batch_statistics') or [0]*24)[6] for s in instances]+[0]),matrix_context_rows=batch[7],exact_numeric_audits=batch[8],completed_binding_shortcuts=batch[12],continuation_blocks=batch[16],union_merge_blocks=batch[17],actual_set_results=batch[18],custom_merge_requests=batch[19],constructor_set_requests=batch[20],constructor_result_audits=batch[23],
        scope='original control gates with witnessed saturation-local successor firing in one shared GPU identity domain',
        cached_spine_reuses=stats[2],spine_builds=stats[3],correlated_rows_built=stats[4],
        correlated_rows_consumed=stats[5],prefix_table_reads=stats[6],invalidated_spines=stats[7],
        cache_compactions=stats[8],witness_validated_tuple_reuses=stats[11],stage_runs=sum(len(s['history']) for s in instances),
        row_index_rebuilds=sum(s['row_index_rebuilds'] for s in instances) if all('row_index_rebuilds' in s for s in instances) else None,
        cache_growths=sum(a.get('region_cache_growths',0) for p in report['programs'] for a in p['attempts']),
        exact_residual_queries=True,invalidation_granularity='table mutation versions',
        identities='shared hashcons/union; local recipe slots receive actual constructor results',
        execution='GPU control, parallel matching over prepared shared contexts, multi-threadgroup matrix evaluation, stable ordered block publication')


def run_artifact(artifact,build,output,check):
    if str(ROOT/'experiments/device_backend') not in sys.path:sys.path.insert(0,str(ROOT/'experiments/device_backend'))
    from run_generic_cells import run_library
    from .artifacts import validate_recipe_artifact
    import subprocess
    import tempfile
    manifest=json.loads((artifact/'manifest.json').read_text());program=json.loads((artifact/'program.json').read_text())
    core=json.loads((artifact/'core.json').read_text());source=artifact/'source_sk_mk.egg'
    # Compare the typed source in a fresh frontend, not just two mutually
    # consistent editable JSON documents. No matching or lowering is performed.
    with tempfile.TemporaryDirectory() as d:
        with (Path(d)/'frontend.log').open('w') as log:
            raw=subprocess.run([str(build/'frontend'),str(source)],input='',text=True,stdout=subprocess.PIPE,stderr=log,check=True)
        current=json.loads(raw.stdout.splitlines()[0])
    if current!=core:raise ValueError('compiled region source/core differs from native frontend')
    validate_recipe_artifact(artifact,manifest,program,core)
    seed=core.pop('initial')
    lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=seed)])
    result=run_library(lib,build,output,program['capacity'],program['match_capacity'],
        marker_program=json.loads((artifact/'marker-program.json').read_text()),compiled_program=artifact/'program.json',audit_numeric=check)
    if not result['complete']:raise ValueError('region program did not complete')
    folder=output/'program-0/cell-0'
    result.update(complete_schedule=True,checked=check,
        inference_seconds=sum(p['inference_seconds'] for p in result['programs']),
        region_execution=execution_summary(result))
    if check:
        repeated=run_library(lib,build,output/'repeat',program['capacity'],program['match_capacity'],marker_program=json.loads((artifact/'marker-program.json').read_text()),compiled_program=artifact/'program.json',audit_numeric=True)
        result['verification']=verify_repeat(result,repeated)
    (output/'report.json').write_text(json.dumps(result,indent=2)+'\n')
    return result


def verify_repeat(first,second):
    """L1 repeatability and in-engine proof/audit checks, not CPU endpoint parity."""
    if not first['complete'] or not second['complete']:raise ValueError('incomplete verification run')
    def instances(r):return [s for p in r['programs'] for s in p['instances']]
    a,b=instances(first),instances(second)
    if len(a)!=len(b):raise ValueError('instance count differs')
    rounds=0
    for x,y in zip(a,b):
        if x['meta'][4] or y['meta'][4]:raise ValueError('GPU execution/audit error')
        if x['history']!=y['history']:raise ValueError('GPU round history or state fingerprints are not reproducible')
        for field in ('rows','parents','fact_parents','sets','watermarks','meta','state','batch_statistics','region_statistics'):
            if x[field]!=y[field]:raise ValueError('GPU final '+field+' is not reproducible')
        if x.get('row_index_rebuilds')!=y.get('row_index_rebuilds'):raise ValueError('GPU index maintenance is not reproducible')
        stats=x.get('batch_statistics') or []
        if stats and stats[14]!=1:raise ValueError('numeric effect audit was not enabled')
        if len(stats)<23 or stats[22]!=len(x['history']):raise ValueError('missing GPU round fingerprints')
        if any(len(h)!=12 for h in x['history']):raise ValueError('recompile artifact for per-round state fingerprints')
        rounds+=len(x['history'])
    return dict(contract='checked derivations and GPU L1 reproducibility; no CPU final-graph completeness claim',
        seed_conditions='exact existing GPU matcher against the Run snapshot',
        successor_conditions='all actual canonical table rows and guards checked before publication; unions merge at block boundaries then resume saved contexts',
        numeric_effects='source i64 arithmetic audited before publication; bounded affine matrix path with integer fallback',
        constructor_results='actual keyed row or its queued constructor-prediction equality; audited before successor use',
        completed=True,rounds=rounds,per_round_fingerprints_and_control_equal=True,
        fingerprint_scope='global resident rows at ordinary Run boundaries; cumulative private fact-coordinate fingerprints inside certified schedule regions',
        final_rows_ids_values_visibility_equal=True,
        native_whole_graph_equivalence_required=False)


def summarize_run(folder):
    """Small, source-linked report from an existing portable run, no execution."""
    import hashlib
    from collections import Counter
    folder=Path(folder).resolve();report=json.loads((folder/'report.json').read_text())
    if not report.get('complete_schedule'):raise ValueError('incomplete source schedule')
    if report.get('checked'):
        repeated=json.loads((folder/'repeat/results.json').read_text())
        verification=verify_repeat(report,repeated)
        del repeated
    else:verification=None
    first=folder/'program-0/cell-0'
    program=json.loads((first/'program.json').read_text());core=json.loads((first/'core.json').read_text())
    state=report['programs'][0]['instances'][0];stages={}
    for h in state['history']:
        name=core['rulesets'][h[0]]['name'];d=stages.setdefault(name,dict(runs=0,matcher_binding_outputs=0))
        d['runs']+=1;d['matcher_binding_outputs']+=h[2]
    by_table=Counter(core['tables'][r[0]]['name'] for r in state['rows'])
    classes=set()
    for row in state['rows']:
        for sort,value in zip(core['tables'][row[0]]['schema'],row[4:]):
            if sort['kind']=='eq':classes.add((sort['name'],value))
    batch=program['region_program']['batch_program'];templates=batch['templates']
    result=dict(schema='eggfuse-resident-run-summary/v1',run=str(folder),
        typed_core_sha256=hashlib.sha256(json.dumps(core,sort_keys=True).encode()).hexdigest(),
        marked_source_sha256=program['marker_program']['source_sha256'],
        worker_provenance=report['worker_provenance'],verification=verification,
        complete_schedule=True,rows=len(state['rows']),equality_classes_in_rows=len(classes),
        stage_runs=stages,rows_by_table=dict(sorted(by_table.items())),
        static_recipes=dict(scope_templates=len(templates),multi_step_templates=sum(len(t['steps'])>1 for t in templates),
            unique_device_programs=len(program['batch_layout']['templates']),
            note='static recipes, not distinct Cell families or runtime coverage'),
        execution=execution_summary(report),
        diagnostic_inference_seconds=report['inference_seconds'],
        shader_compile_seconds=sum(a['shader_compile_seconds'] for x in report['programs'] for a in x['attempts']),
        timing_scope='checked diagnostic with per-round GPU fingerprints; not a clean benchmark or speedup',
        remaining='exact residual queries for ambiguous/recursive/unsupported compositions; preparation, sorting and keyed publication remain serial per arena')
    (folder/'summary.json').write_text(json.dumps(result,indent=2)+'\n');return result

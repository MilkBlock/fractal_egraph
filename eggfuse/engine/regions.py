"""Execute a verified analyzer region artifact. No terms-specific runtime path."""
import json
import sys
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]


def execution_summary(report):
    instances=[s for p in report['programs'] for s in p['instances']]
    batch=[sum((s.get('batch_statistics') or [0]*16)[i] for s in instances) for i in range(16)]
    stats=[sum(s.get('region_statistics',[0]*16)[i] for s in instances) for i in range(16)]
    matched=sum(h[2] for s in instances for h in s['history'])
    return dict(matcher_binding_outputs=matched,executed_rule_invocations=matched+batch[2],match_count_scope='matcher outputs plus separately counted accepted inline invocations; neither count is committed mutations',batch_tiles=batch[0],batch_seed_invocations=batch[1],validated_successor_invocations=batch[2],rejected_successor_invocations=batch[3],batch_effect_requests=batch[4],residual_invocations=batch[5],max_tile_rows=max([(s.get('batch_statistics') or [0]*16)[6] for s in instances]+[0]),matrix_context_rows=batch[7],exact_numeric_audits=batch[8],completed_binding_shortcuts=batch[12],
        scope='original control gates with witnessed saturation-local successor firing in one shared GPU identity domain',
        cached_spine_reuses=stats[2],spine_builds=stats[3],correlated_rows_built=stats[4],
        correlated_rows_consumed=stats[5],prefix_table_reads=stats[6],invalidated_spines=stats[7],
        cache_compactions=stats[8],witness_validated_tuple_reuses=stats[11],stage_runs=sum(len(s['history']) for s in instances),
        cache_growths=sum(a.get('region_cache_growths',0) for p in report['programs'] for a in p['attempts']),
        exact_residual_queries=True,invalidation_granularity='table mutation versions',
        identities='shared hashcons/union; local recipe slots receive actual constructor results',
        execution='GPU control, multi-threadgroup matrix/context evaluation, stable ordered block publication')


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
        stats=x.get('batch_statistics') or []
        if stats and stats[14]!=1:raise ValueError('numeric effect audit was not enabled')
        if any(len(h)!=12 for h in x['history']):raise ValueError('recompile artifact for per-round state fingerprints')
        rounds+=len(x['history'])
    return dict(contract='checked derivations and GPU L1 reproducibility; no CPU final-graph completeness claim',
        seed_conditions='exact existing GPU matcher against the Run snapshot',
        successor_conditions='all actual canonical table rows and guards checked before publication; pending unions defer successor',
        numeric_effects='source i64 arithmetic audited before publication; bounded affine matrix path with integer fallback',
        completed=True,rounds=rounds,per_round_fingerprints_and_control_equal=True,final_rows_ids_values_visibility_equal=True,
        native_whole_graph_equivalence_required=False)

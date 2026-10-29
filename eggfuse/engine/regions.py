"""Execute a verified analyzer region artifact. No terms-specific runtime path."""
import json
import sys
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]


def execution_summary(report):
    instances=[s for p in report['programs'] for s in p['instances']]
    stats=[sum(s.get('region_statistics',[0]*16)[i] for s in instances) for i in range(16)]
    return dict(scope='shared global GPU identity domain with original staged control',
        cached_spine_reuses=stats[2],spine_builds=stats[3],correlated_rows_built=stats[4],
        correlated_rows_consumed=stats[5],prefix_table_reads=stats[6],invalidated_spines=stats[7],
        cache_compactions=stats[8],witness_validated_tuple_reuses=stats[11],stage_runs=sum(len(s['history']) for s in instances),
        cache_growths=sum(a.get('region_cache_growths',0) for p in report['programs'] for a in p['attempts']),
        exact_residual_queries=True,invalidation_granularity='table mutation versions',
        identities='shared hashcons/union; local recipe slots receive actual constructor results',
        execution='GPU-resident control, serial within a shared arena; not a throughput claim')


def run_artifact(artifact,build,output,check):
    if str(ROOT/'experiments/device_backend') not in sys.path:sys.path.insert(0,str(ROOT/'experiments/device_backend'))
    from run_generic_cells import run_library
    from .artifacts import validate_recipe_artifact
    from verify_native_source import verify as verify_native
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
        marker_program=json.loads((artifact/'marker-program.json').read_text()),compiled_program=artifact/'program.json')
    if not result['complete']:raise ValueError('region program did not complete')
    folder=output/'program-0/cell-0'
    result.update(complete_schedule=True,checked=check,
        inference_seconds=sum(p['inference_seconds'] for p in result['programs']),
        region_execution=execution_summary(result))
    if check:result['verification']=verify_native(source,build,folder,output/'native')
    (output/'report.json').write_text(json.dumps(result,indent=2)+'\n')
    return result

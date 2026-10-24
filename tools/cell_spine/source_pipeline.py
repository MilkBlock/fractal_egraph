"""SK source -> measured plan selection -> executable markers + inline Cells.

This stage has exactly one program input. All pricing is collected by replaying
that input. Files in output are diagnostics, never dependencies of the final egg.
"""
import argparse
import gzip
import json
import os
from pathlib import Path
import subprocess
from . import compiler, scheduled, selected_source, solver, workload, contexts


def run(source,output,backend='scip',profile_binary=None):
    source=Path(source).resolve();output=Path(output).resolve()
    output.mkdir(parents=True,exist_ok=False)
    text=source.read_text()
    if not any(s.startswith('; cell-structural-key-runtime/v1 ') for s in text.splitlines()):
        raise ValueError('marker stage requires source_sk.egg from the structural-key stage')
    if any(s.startswith('; cell-runtime/v1 ') for s in text.splitlines()):
        raise ValueError('input already contains runtime Cell declarations')
    # No inherited sidecar/closure switches influence compilation or profiling.
    env={k:v for k,v in os.environ.items() if not k.startswith(('CELL_','EGGLOG_'))}
    env['RAYON_NUM_THREADS']='1'
    native=compiler.ROOT/'target/analyzer/release/examples/scheduled_spine'
    profile=profile_binary or compiler.ROOT/'target/analyzer/release/examples/planner_profile'
    def invoke(args):subprocess.run([str(x) for x in args],cwd=compiler.ROOT,env=env,check=True)
    inventory=output/'inventory.json';invoke([native,source,inventory,'--all-schedules'])
    data=json.loads(inventory.read_text())
    trace=output/'profile.jsonl.gz'
    with (output/'profile.log').open('wb') as log, gzip.open(trace,'wb') as target:
        p=subprocess.Popen([str(profile),str(source),'-'],cwd=compiler.ROOT,env=env,stdout=subprocess.PIPE,stderr=log)
        try:
            for chunk in iter(lambda:p.stdout.read(1024*1024),b''):target.write(chunk)
        finally:p.stdout.close()
        if p.wait():raise ValueError('source profiling failed; see profile.log; no incomplete profile is priced')
    source_hash=compiler.digest(source)
    manifest=dict(schema='marker-workload-corpus/v1',complete=True,
        identity=dict(workloads=[dict(source=str(source),sha256=source_hash)]),
        cases=[dict(source=str(source),source_sha256=source_hash,status='complete',passes=[dict(
            name='source_sk',status='complete',profile=str(trace),profile_sha256=compiler.digest(trace),
            input=str(source),input_sha256=source_hash)])])
    file=output/'profile-manifest.json';scheduled.write(file,manifest)
    measured=workload.collect(file)
    # Use only native queries whose columns the event mirror admits. Unsupported
    # source rules remain executable on the ordinary backend, never disappear.
    queries=[q for q in data['queries'] if 'plans' in q and workload.query_key(q) in measured['queries']]
    solver.select(backend)
    decision=scheduled.select(queries,data['table_rows'],output,[1,1,1],runtime_workload=measured)
    scheduled.write(output/'selection.json',decision)
    scheduled.write(output/'context-domain.json',contexts.discover(queries))
    layout=output/'selected.json'
    scheduled.write(layout,dict(schema='cell-boundary-programs/v2',setup=data['preserved_commands'][:data['setup_commands']],
        rules=[dict(ruleset=q['ruleset'],rule=q['rule'],root=q['anchor_table'],plan=q['plan']) for q in decision['selected']]))
    lowered=selected_source.compile_source(source,layout,output/'lowered',joint=False)
    final=output/'source_sk_mk.egg'
    final.write_bytes((output/'lowered/source_sk_mk.egg').read_bytes())
    report=dict(schema='self-contained-marker-stage/v1',input_sha256=source_hash,output_sha256=compiler.digest(final),
        only_input=str(source),pricing='verified execution of preceding source_sk.egg; no external corpus or static prices',
        selected_queries=len(decision['selected']),lowered_queries=len(lowered['lowered_queries']),
        retained_query_contexts=lowered['runtime_rule_count'],marker_tables=sum(not m['alias'] for m in lowered['markers']),
        rhs_publications=lowered['publish_actions'],solver=solver.statistics(),
        guarantee='complete typed query plans; original effects and Run boundaries; no inferred saturation certificate',
        limitations=['Unsupported queries retain original execution. Profiling is offline compilation cost.',
            'Selection minimizes measured work estimates, not elapsed time; no speedup is implied.'])
    scheduled.write(output/'report.json',report)
    return report

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--solver',choices=['scip','gurobi'],default='scip')
    a=p.parse_args();print(json.dumps(run(a.source,a.output,a.solver)))

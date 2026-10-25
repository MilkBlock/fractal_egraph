"""Single-run native port regression diagnosis, reusing exact completed lanes.

No new native profiler, solver, graph dump, Tiger invocation, or GPU worker.
Existing RunReport clocks attribute native work; they omit mixed Cell batches.
This does not certify changed source policies as equivalent.
"""
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import time


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def read(path):
    return json.loads(Path(path).read_text())


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + '\n')


def seconds(value):
    return value['secs'] + value['nanos'] * 1e-9


def native_breakdown(report):
    groups = {}
    for kind in ['search_and_apply', 'merge', 'rebuild']:
        for name, value in report[kind + '_time_per_ruleset'].items():
            groups.setdefault(name, {})[kind] = seconds(value)
    return sorted([dict(ruleset=n, **d, total=sum(d.values())) for n, d in groups.items()],
                  key=lambda d: -d['total'])


def completed(path, identity):
    file = path / 'lane.json'
    if not file.is_file():
        return False
    old = read(file)
    return (old.get('identity') == identity and old.get('returncode') == 0
            and all((path / n).is_file() and sha(path / n) == digest
                    for n, digest in old.get('files', {}).items())
            and {'timing.json', 'runtime.json'} <= old.get('files', {}).keys())


def run(a, resolve):
    root, runner, out = resolve(a.fork), resolve(a.runner), resolve(a.output)
    inputs = {n: resolve(getattr(a, n.replace('-', '_')))
              for n in ['before-sk', 'before-mk', 'after-sk', 'after-mk']}
    inputs['before-query-only'] = inputs['before-mk']
    if a.dry_run:
        print([(n, str(inputs[n])) for n in a.lanes])
        return 0
    out.mkdir(parents=True, exist_ok=True)
    frozen = out / 'runner'
    if not frozen.exists():
        shutil.copy2(runner, frozen)
    if sha(frozen) != sha(runner):
        raise ValueError('runner changed; choose a new output directory')
    results = []
    for name in a.lanes:
        env = {k: v for k, v in os.environ.items() if not k.startswith(('CELL_', 'EGGLOG_'))}
        env.update(RAYON_NUM_THREADS='1', CELL_NATIVE_MODE='record',
                   CELL_NATIVE_PROBE_COUNTS=a.probe_counts)
        if name == 'before-query-only':
            env['CELL_NATIVE_CLOSURES'] = 'off'
        identity = dict(schema='native-port-lane/v1', binary_sha256=sha(frozen),
                        source_sha256=sha(inputs[name]), environment={k:v for k,v in env.items()
                            if k.startswith(('CELL_','EGGLOG_','RAYON_')) or k == 'RUST_LOG'},
                        machine=platform.node(), platform=platform.platform())
        folder = out / name
        if not completed(folder, identity):
            if folder.exists():
                raise ValueError('incomplete or changed lane; preserve it and choose a new output: ' + str(folder))
            folder.mkdir()
            env.update(CELL_NATIVE_CACHE=str(folder/'cache'), CELL_NATIVE_RUN_REPORT=str(folder/'timing.json'))
            command = [str(frozen), str(inputs[name])]
            with (folder/'run.log').open('w') as log:
                start = time.perf_counter()
                result = subprocess.run(command, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT)
                wall = time.perf_counter() - start
            lane = dict(identity=identity, command=command, returncode=result.returncode,
                        wall_seconds=wall, measured_at=time.strftime('%Y-%m-%dT%H:%M:%S%z'),
                        source=str(inputs[name]), fork_head=subprocess.check_output(
                            ['git','rev-parse','HEAD'],cwd=root,text=True).strip(),
                        working_diff_sha256=hashlib.sha256(subprocess.check_output(['git','diff'],cwd=root)).hexdigest())
            if result.returncode == 0:
                packets = list((folder/'cache').glob('*/record-mk-full.json'))
                if len(packets) != 1:
                    raise ValueError('expected exactly one source execution report')
                shutil.copy2(packets[0], folder/'runtime.json')
                lane['files'] = {n: sha(folder/n) for n in ['timing.json','runtime.json']}
            write(folder/'lane.json', lane)
            if result.returncode:
                raise ValueError('lane failed: ' + str(folder/'run.log'))
        else:
            print('Reused completed lane:', name, flush=True)
        meta, timing, runtime = [read(folder/n) for n in ['lane.json','timing.json','runtime.json']]
        stats = runtime['runtime']
        compact = dict(lane=name, execution_seconds=runtime['execution_seconds'],
                       parse_and_execute_seconds=timing['parse_and_execute_seconds'],
                       wall_seconds=meta['wall_seconds'],
                       tuples_including_auxiliary=runtime['public_and_auxiliary_tuples'],
                       native_breakdown_scope=timing['native_report_scope'],
                       native_rulesets=native_breakdown(timing),
                       runtime={k:v for k,v in stats.items() if not isinstance(v,(list,dict))},
                       provenance=meta)
        results.append(compact)
        write(out/'report.json', dict(schema='native-port-regression/v1', gpu=False,
              scope='Saved full Pass-3 program in actual native runtime; no extraction or graph export. Before/after source policies differ.',
              correctness_revalidated=False, lanes=results))
        print(name, round(runtime['execution_seconds'], 6), 's;',
              stats.get('runs'), 'Runs;', stats.get('skipped_saturations'), 'sat skips', flush=True)
    return 0

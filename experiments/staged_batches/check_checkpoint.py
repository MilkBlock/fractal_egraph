"""Real Metal checkpoint/restart checks; no prefix replay or mocked GPU state."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT), str(ROOT / 'experiments/device_backend')]
from test_generic_cells import core_for
from run_generic_cells import run_library, build_worker
from eggfuse.analyzer.regions import from_core
from eggfuse.engine.replay import replay


def check(build, out):
    out.mkdir(parents=True, exist_ok=False)
    cases = {
        'chain': (ROOT / 'experiments/staged_batches/chain.egg').read_text(),
        'union-merge-subsume': '''(datatype E (N i64) (K E) (Box E) (Choice E E))
(function F (E) E :merge (Choice old new))
(relation Seed (E E)) (relation Seen (E E)) (ruleset a) (ruleset b)
(rule ((Seed x y)) ((K x) (K y) (Box x) (set (F x) y) (union x y)) :ruleset a)
(rule ((= z (Choice x y))) ((Seen x y) (subsume (Choice x y))) :ruleset b)
(Seed (N 0) (N 1)) (set (F (N 0)) (N 2))
(run-schedule (repeat 2 (run a) (run b)))''',
        'growing-history-and-contexts': '''(datatype E (N i64)) (relation Seed (E i64))
(relation Mid (E i64)) (relation Done (E i64)) (ruleset a) (ruleset b)
(rule ((Seed r n)) ((Mid r (+ n 1))) :ruleset a)
(rule ((Mid r n)) ((Done r (+ n 2))) :ruleset b)
(relation Tick (i64)) (Tick 0)
(rule ((Tick n) (< n 20)) ((Tick (+ n 1))) :ruleset a)
''' + ''.join(f'(Seed (N {i}) {i})\n' for i in range(257)) + '(run-schedule (saturate (run a) (run b)) (repeat 10 (run a) (run b)))',
        'block-continuation': '''(datatype E (N i64) (Edge E)) (relation Seed (E E))
(relation Seen (E)) (relation Done (E)) (ruleset a) (ruleset b) (ruleset c)
(rule ((Seed x y)) ((union (Edge x) y)) :ruleset a)
(rule ((= y (Edge x))) ((Seen y)) :ruleset b)
(rule ((Seen y)) ((Done y)) :ruleset c)
(Seed (N 0) (N 2)) (Edge (N 0))
(run-schedule (saturate (run a) (run b) (run c)))''',
    }
    results = {}
    for name, text in cases.items():
        source = out / (name + '.egg'); source.write_text(text)
        core = core_for(source, build); plan = from_core(core); initial = core.pop('initial')
        lib = dict(schema='analyzer-device-cells/v1', programs=[core],
                   instances=[dict(unit=0, program=0, initial=initial)])
        run_library(lib, build, out / (name+'-full'), capacity=4096, match_capacity=2048,
                    region_program=plan, audit_numeric=True)
        program = out / (name+'-full/program-0/attempt-0/program.json')
        expected = json.loads(program.with_name('result.json').read_text())
        assert all(s['completed'] for s in expected['instances'])
        stops = [0, 1, 17] if name=='growing-history-and-contexts' else [0, 1, 2]
        checkpoint = None
        for j, stop in enumerate(stops):
            folder = out / f'{name}-at-{stop}'
            summary = replay(program, build, folder, checkpoint, stop, quantum=3+j)
            state = json.loads((folder/'result.json').read_text())
            assert summary['completed_runs'] == [stop] and summary['paused']
            assert state['instances'][0]['history'] == expected['instances'][0]['history'][:stop]
            assert state['instances'][0]['meta'][6] == stop
            checkpoint = folder / 'checkpoint'
        folder = out / (name+'-resumed')
        summary = replay(program, build, folder, checkpoint, quantum=2)
        actual = json.loads((folder/'result.json').read_text())
        assert summary['complete'] and not summary['paused']
        assert expected['instances'] == actual['instances'], name
        if name=='block-continuation':
            assert actual['instances'][0]['batch_statistics'][16] > 0
        if name=='growing-history-and-contexts':
            assert actual['batch_context_growths'] > 0
            assert json.loads((checkpoint/'manifest.json').read_text())['sizes']['history_capacity'] > 16
        results[name] = dict(completed_runs=len(actual['instances'][0]['history']),
            suspension_points=stops, full_rows_ids_values_visibility_equal=True,
            per_round_fingerprints_and_control_equal=True, effect_and_index_counters_equal=True)
        print(name, 'checkpoint/resume verified', flush=True)
    # Fail closed before dispatch for changed programs and damaged state files.
    worker = build_worker(build); bad = json.loads(program.read_text()); bad['batch_audit_numeric'] = False
    changed = out/'wrong-program.json'; changed.write_text(json.dumps(bad))
    def rejected(path, label):
        with (out/(label+'.log')).open('w') as log:
            r = subprocess.run([str(worker), str(path), str(out/(label+'.json')), '--resume', str(checkpoint)], stdout=log, stderr=subprocess.STDOUT)
        assert r.returncode != 0 and not (out/(label+'.json')).exists(), label
    rejected(changed, 'changed-program')
    buf = checkpoint/'uints.bin'
    with buf.open('r+b') as f:
        byte = f.read(1); f.seek(0); f.write(bytes([byte[0]^1]))
    rejected(program, 'corrupt-buffer')
    with buf.open('r+b') as f: f.write(byte)
    # The checkpoint carries its exact worker, independent of this worktree's
    # later source or executable. No replacement binary is built on resume.
    fresh=out/'absent-build';assert not fresh.exists()
    frozen=replay(program,fresh,out/'frozen-worker-resumed',checkpoint)
    assert frozen['complete'] and frozen['worker_source']=='checkpoint image'
    assert not (fresh/'generic-cell-worker').exists()
    image=checkpoint/'worker'
    with image.open('r+b') as f:
        byte=f.read(1);f.seek(0);f.write(bytes([byte[0]^1]))
    try:
        try:replay(program,fresh,out/'corrupt-worker',checkpoint)
        except ValueError as error:assert 'frozen worker checksum' in str(error)
        else:raise AssertionError('corrupted worker executed')
    finally:
        with image.open('r+b') as f:f.write(byte)
    results['negative_checks'] = dict(changed_program_rejected=True, corrupt_buffer_rejected=True,corrupt_worker_rejected=True,frozen_resume_without_current_build=True)
    (out/'results.json').write_text(json.dumps(results, indent=2)+'\n')
    return results


if __name__=='__main__':
    p = argparse.ArgumentParser(); p.add_argument('--build', type=Path, required=True); p.add_argument('--output', type=Path, required=True);p.add_argument('--incremental-rebuild',action='store_true')
    a = p.parse_args()
    if a.incremental_rebuild:
        import run_generic_cells as runtime
        original=runtime.lower
        runtime.lower=lambda *args,**kwargs:original(*args,**kwargs,incremental_rebuild=True)
    check(a.build.resolve(), a.output.resolve())

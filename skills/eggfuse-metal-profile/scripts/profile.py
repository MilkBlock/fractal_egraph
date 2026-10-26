#!/usr/bin/env python3
"""Xcode diagnostics for a saved eggfuse/device_backend Run stream.

Does not change the engine, compiler, program or schedule. A separate Swift
worker adds labels and optional one-Run Metal capture. Diagnostics are never
timing baselines. See SKILL.md for interpretation and validation boundaries.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shutil
import signal
import sqlite3
import struct
import subprocess
import time
import uuid
import xml.etree.ElementTree as ET


def sha(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def write(path, data):
    Path(path).write_text(json.dumps(data, indent=2) + '\n')


def command(args, **kwargs):
    return subprocess.run(list(map(str, args)), check=True, **kwargs)


def fresh(path):
    path = Path(path).resolve()
    path.mkdir(parents=True, exist_ok=False)
    return path


def replace_once(text, before, after):
    if text.count(before) != 1:
        raise ValueError('worker ABI changed; missing/ambiguous instrumentation anchor: ' + before[:90])
    return text.replace(before, after, 1)


def hotspots(a):
    report = json.loads((a.run / 'report.json').read_text())
    phases = {}
    for key in ['prepare_seconds', 'match_seconds', 'rhs_commit_seconds', 'parallel_rebuild_seconds']:
        rows = [(i+1, r) for i, r in enumerate(report['device_command_times']) if r.get(key) is not None]
        phases[key] = {'total_seconds': sum(r[key] for _, r in rows),
                      'slow_runs': [{'run': i, 'ruleset': r['set'], 'seconds': r[key],
                                     'rows_after': report['steps'][i-1]['rows'],
                                     'logical_matches': report['steps'][i-1]['matched']}
                                    for i,r in sorted(rows, key=lambda x: -x[1][key])[:5]]}
    print(json.dumps({'saved_run': str(a.run.resolve()), 'source_sha256': report['source_sha256'],
                      'inference_seconds': report.get('inference_seconds'),
                      'phases': phases, 'note': 'Existing timers guide capture selection; no new benchmark.'}, indent=2))


def prepare(a):
    out = fresh(a.output)
    report = json.loads((a.run / 'report.json').read_text())
    text = a.source.read_text()
    expected = [v for k, v in report['build']['sources'].items() if k.endswith('/worker.swift')]
    if expected != [sha(a.source)]:
        raise ValueError('worker source does not match the saved run; supply its original source')
    labels = ['var profileNames: [ObjectIdentifier: String] = [:]']
    for var, name in re.findall(r'let (\w+)=try device.makeComputePipelineState\(function:lib.makeFunction\(name:"([^"]+)"\)!\)', text):
        labels.append(f'profileNames[ObjectIdentifier({var})] = "{name}"')
    for var, names in re.findall(r'let (\w+)=try (\["[^\n]+?\])\.map\{try device.makeComputePipelineState', text):
        json.loads(names)  # Require a literal list, not arbitrary source injection.
        labels.append(f'for (p,n) in zip({var}, {names}) {{ profileNames[ObjectIdentifier(p)] = n }}')
    labels.append('for (p,n) in zip(stepPipelines, program["kernel_names"] as! [String]) { profileNames[ObjectIdentifier(p)] = n }')
    text = replace_once(text, 'let capacity=program["capacity"] as! Int', '\n'.join(labels) + '\nlet capacity=program["capacity"] as! Int')
    text = replace_once(text, 'let e=cb.makeComputeCommandEncoder()!;e.setComputePipelineState(pipeline)', '''let e=cb.makeComputeCommandEncoder()!;e.setComputePipelineState(pipeline)
 let pn=profileNames[ObjectIdentifier(pipeline)] ?? "unknown"
 e.label="fn=\\(pn) threads=\\(threads) indirect=\\(indirect)"
 if cb.label == nil { cb.label="run=\\(hostMeta[6]+1) set=\\(arg) first=\\(pn)" }''')
    text = replace_once(text, 'if op==1{', '''if op==1{
  let captureStep=UInt32(ProcessInfo.processInfo.environment["EGGFUSE_CAPTURE_RUN"] ?? "0") ?? 0
  let captureThisRun=(hostMeta[6]+1 == captureStep)
  if captureThisRun {
   let descriptor=MTLCaptureDescriptor();descriptor.captureObject=queue
   descriptor.destination = .gpuTraceDocument
   descriptor.outputURL=URL(fileURLWithPath:ProcessInfo.processInfo.environment["EGGFUSE_CAPTURE_PATH"]!)
   try MTLCaptureManager.shared().startCapture(with:descriptor)
  }''')
    text = replace_once(text, '}else if op==2{', ' if captureThisRun { MTLCaptureManager.shared().stopCapture() }\n }else if op==2{')
    (out / 'worker.swift').write_text(text)
    command(['xcrun', 'swiftc', '-O', out / 'worker.swift', '-o', out / 'worker'])
    write(out / 'build.json', {'original_worker_source_sha256': sha(a.source),
          'source': str(a.source.resolve()), 'source_sha256': sha(out / 'worker.swift'),
          'worker_sha256': sha(out / 'worker'), 'saved_run': str(a.run.resolve()),
          'modifications': 'Metal encoder/command labels and optional one-Run capture only'})
    print(out)


class Notification:
    """The documented xctrace readiness notification, not English console text."""
    def __init__(self):
        self.name = 'org.eggfuse.profile.' + uuid.uuid4().hex
        self.lib = ctypes.CDLL(None)
        self.token = ctypes.c_int()
        status = self.lib.notify_register_check(self.name.encode(), ctypes.byref(self.token))
        if status:
            raise RuntimeError(f'notify_register_check failed: {status}')
        self.ready()  # Clear the initial registration notification.

    def ready(self):
        flag = ctypes.c_int()
        if self.lib.notify_check(self.token, ctypes.byref(flag)):
            raise RuntimeError('notify_check failed')
        return bool(flag.value)

    def close(self):
        self.lib.notify_cancel(self.token)


def receive(pipe, n, timeout=300):
    data = bytearray()
    deadline = time.monotonic() + timeout
    while len(data) < n:
        ready, _, _ = select.select([pipe], [], [], max(0, deadline - time.monotonic()))
        if not ready:
            raise TimeoutError('worker protocol timed out')
        chunk = os.read(pipe.fileno(), n - len(data))
        if not chunk:
            raise RuntimeError('worker ended during protocol; inspect worker.log')
        data.extend(chunk)
    return data


def issues(trace):
    result = []
    for p in trace.glob('*.run/RunIssues.storedata'):
        with sqlite3.connect('file:' + str(p) + '?mode=ro', uri=True) as db:
            result.extend({'type': t, 'subtype': s, 'message': m}
                          for t, s, m in db.execute('SELECT ZTYPE,ZSUBTYPE,ZMESSAGE FROM ZISSUE'))
    return result


def record(a):
    out = fresh(a.output)
    source = a.run.resolve()
    report = json.loads((source / 'report.json').read_text())
    program = json.loads((source / 'program.json').read_text())
    build = json.loads((a.build / 'build.json').read_text())
    if build['worker_sha256'] != sha(a.build / 'worker'):
        raise ValueError('profile worker fingerprint mismatch')
    if build['original_worker_source_sha256'] not in report['build']['sources'].values():
        raise ValueError('profile worker does not belong to this Run stream')
    if report.get('resumed_prefix_runs', 0):
        raise ValueError('use a fresh complete Run stream, not a continuation')
    if sha(source / 'program.json') != report['program_sha256']:
        raise ValueError('saved program changed')
    end = a.end_run or len(report['steps'])
    if not 0 <= a.start_after < end <= len(report['steps']):
        raise ValueError('invalid recording Run range')
    env = dict(os.environ)
    if a.mode == 'capture':
        env.update(METAL_CAPTURE_ENABLED='1', EGGFUSE_CAPTURE_RUN=str(a.start_after + 1),
                   EGGFUSE_CAPTURE_PATH=str(out / 'hotspot.gputrace'))
    provenance = {'scope': 'instrumented GPU replay; not a benchmark or proof of native equality',
                  'saved_run': str(source), 'program_sha256': sha(source / 'program.json'),
                  'original_worker_sha256': report['build']['worker_sha256'], 'profile_build': build,
                  'mode': a.mode, 'start_after': a.start_after, 'end_run': end,
                  'seconds': a.seconds, 'template': a.template,
                  'waiting_threads': a.waiting_threads,
                  'disk_free_before': shutil.disk_usage(out).free,
                  'xcode': subprocess.check_output(['xcodebuild', '-version'], text=True).strip()}
    write(out / 'provenance.json', provenance)
    trace = None
    notification = None
    matched = 0
    phases = []
    worker = None
    try:
        with (out / 'worker.log').open('wb') as log, (out / 'xctrace.log').open('w') as trace_log:
            worker = subprocess.Popen([str((a.build / 'worker').resolve()), str(source / 'program.json')],
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, env=env, bufsize=0)
            print('worker', worker.pid, flush=True)
            protocol = bytes(receive(worker.stdout, 4))
            sizes = {b'DVB6': (96, 6), b'DVB7': (100, 6)}
            if protocol not in sizes:
                raise ValueError(f'unsupported protocol {protocol!r}; update the diagnostic adapter')
            size, nphases = sizes[protocol]
            for i, step in enumerate(report['steps'][:end]):
                if i == a.start_after and a.mode != 'capture':
                    notification = Notification()
                    args = ['xcrun', 'xctrace', 'record', '--attach', str(worker.pid),
                            '--output', str(out / 'profile.trace'), '--no-prompt',
                            '--notify-tracing-started', notification.name]
                    if a.template:
                        args += ['--template', a.template]
                    else:
                        args += ['--instrument', 'GPU', '--instrument', 'Metal Application']
                        if a.mode == 'counters':
                            args += ['--instrument', 'Metal GPU Counters']
                    if a.seconds:
                        args += ['--time-limit', f'{round(a.seconds * 1000)}ms']
                    if a.waiting_threads:
                        options = out / 'recording-options.json'
                        option_args = ['--template', a.template] if a.template else ['--instrument', 'Time Profiler']
                        if not a.template:
                            args += ['--instrument', 'Time Profiler']
                        settings = json.loads(subprocess.check_output(
                            ['xcrun', 'xctrace', 'record', *option_args, '--show-recording-options'], text=True))
                        settings['Time Profiler']['recordWaitingThreads'] = True
                        write(options, settings)
                        args += ['--recording-options', str(options)]
                    write(out / 'xctrace-command.json', args)
                    trace = subprocess.Popen(args, stdout=trace_log, stderr=subprocess.STDOUT)
                    until = time.monotonic() + 60
                    while not notification.ready():
                        if trace.poll() is not None or time.monotonic() > until:
                            raise RuntimeError('xctrace did not signal readiness; inspect xctrace.log')
                        time.sleep(.05)
                    print('recording ready at Run', i + 1, flush=True)
                worker.stdin.write(struct.pack('<II', 1, program['sets'].index(step['set'])))
                packet = receive(worker.stdout, size)
                meta = struct.unpack_from('<9I', packet)
                actual = dict(rows=meta[0], matched=meta[5], updated=bool(meta[3]), epoch=meta[2], unions=meta[7])
                if meta[4] or any(actual[k] != step[k] for k in actual):
                    raise RuntimeError(f'Run {i+1} replay differs: error={meta[4]}, actual={actual}, expected={step}')
                matched += 1
                phases.append({'run': i + 1, 'set': step['set'],
                               'seconds': struct.unpack_from('<' + 'd' * nphases, packet, 36)})
                if matched % 1000 == 0:
                    print('verified saved Run metadata', matched, flush=True)
            worker.stdin.write(struct.pack('<II', 0, 0))
            worker.wait(timeout=60)
            if worker.returncode:
                raise RuntimeError(f'worker failed: {worker.returncode}')
            if trace:
                if not a.seconds and trace.poll() is None:
                    trace.send_signal(signal.SIGINT)
                trace.wait(timeout=240)
                if trace.returncode:
                    raise RuntimeError(f'xctrace failed: {trace.returncode}; do not use partial counters')
            write(out / 'replay.json', {'runs': matched, 'saved_run_metadata_equal': True,
                  'complete_schedule': end == len(report['steps']), 'original_graph_checked': False,
                  'diagnostic_only': True, 'worker_pid': worker.pid, 'phases': phases,
                  'trace_issues': issues(out / 'profile.trace') if trace else []})
    except BaseException as e:
        write(out / 'failure.json', {'error': str(e), 'verified_run_metadata': matched, 'diagnostic_only': True})
        raise
    finally:
        if worker and worker.poll() is None:
            worker.terminate()
            worker.wait(timeout=30)
        if trace and trace.poll() is None:
            trace.send_signal(signal.SIGINT)
            trace.wait(timeout=240)
        if notification:
            notification.close()
    print(out, flush=True)


TABLES = ['metal-gpu-intervals', 'metal-application-intervals',
          'metal-application-encoders-list', 'metal-command-buffer-completed',
          'metal-application-command-buffer-submissions', 'metal-gpu-counter-profile',
          'metal-gpu-counter-intervals', 'gpu-counter-info',
          'metal-shader-profiler-intervals',
          'metal-shader-profiler-shader-list', 'time-profile']


def export(a):
    out = fresh(a.output)
    command(['xcrun', 'xctrace', 'export', '--input', a.trace, '--toc', '--output', out / 'toc.xml'])
    schemas = {t.get('schema') for t in ET.parse(out / 'toc.xml').findall('.//table')}
    counts = {}
    for schema in TABLES:
        if schema not in schemas:
            continue
        p = out / (schema + '.xml')
        command(['xcrun', 'xctrace', 'export', '--input', a.trace, '--xpath',
                 f'/trace-toc/run[@number="1"]/data/table[@schema="{schema}"]', '--output', p])
        count = 0
        for _, e in ET.iterparse(p):
            if e.tag == 'row':
                count += 1
                e.clear()
        counts[schema] = count
    write(out / 'availability.json', {'rows': counts, 'trace_issues': issues(a.trace)})
    print(json.dumps(counts, indent=2))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest='command', required=True)
    q = sub.add_parser('hotspots', help='read saved phase/Run timings; do not execute the engine')
    q.add_argument('--run', type=Path, required=True)
    q.set_defaults(func=hotspots)
    q = sub.add_parser('prepare')
    q.add_argument('--run', type=Path, required=True)
    q.add_argument('--source', type=Path, required=True)
    q.add_argument('--output', type=Path, required=True)
    q.set_defaults(func=prepare)
    q = sub.add_parser('record')
    q.add_argument('--run', type=Path, required=True)
    q.add_argument('--build', type=Path, required=True)
    q.add_argument('--output', type=Path, required=True)
    q.add_argument('--mode', choices=['timeline', 'counters', 'capture'], default='timeline')
    q.add_argument('--start-after', type=int, default=0)
    q.add_argument('--end-run', type=int)
    q.add_argument('--seconds', type=float, default=3, help='bounded trace window; 0 explicitly requests the whole remaining stream (very large)')
    q.add_argument('--template', help='optional installed/custom Xcode template')
    q.add_argument('--waiting-threads', action='store_true', help='include sleeping threads in the template Time Profiler')
    q.set_defaults(func=record)
    q = sub.add_parser('export')
    q.add_argument('--trace', type=Path, required=True)
    q.add_argument('--output', type=Path, required=True)
    q.set_defaults(func=export)
    a = p.parse_args()
    a.func(a)


if __name__ == '__main__':
    main()

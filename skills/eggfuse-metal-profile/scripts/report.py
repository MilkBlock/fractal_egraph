#!/usr/bin/env python3
"""Read xctrace exports without confusing queue activity with occupancy."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
import re
import xml.etree.ElementTree as ET


def tables(path):
    """xctrace IDs are scoped to one export, including nested process elements."""
    if not path.exists():
        return []
    root = ET.parse(path).getroot()
    refs = {e.get('id'): e for e in root.iter() if e.get('id')}

    def decode(e):
        seen = set()
        while e.get('ref'):
            key = e.get('ref')
            if key in seen or key not in refs:
                raise ValueError('invalid xctrace reference: ' + key)
            seen.add(key)
            e = refs[key]
        return {'raw': e.text or '', 'fmt': e.get('fmt', e.text or '')}

    result = []
    for node in root.findall('node'):
        cols = [c.findtext('mnemonic') for c in node.findall('schema/col')]
        rows = []
        for row in node.findall('row'):
            if len(row) != len(cols):
                raise ValueError('xctrace row width differs from schema')
            rows.append(dict(zip(cols, map(decode, row))))
        result.append({'schema': node.find('schema').get('name'), 'columns': cols, 'rows': rows})
    return result


def intervals_union(intervals):
    merged = []
    for lo, hi in sorted(intervals):
        if hi < lo:
            raise ValueError('negative GPU interval')
        if merged and lo <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], hi)
        else:
            merged.append([lo, hi])
    return merged


def trace_problems(issues):
    # Xcode 27 writes this stream header even for successful, complete traces.
    # The SQLite ZTYPE value is not a documented severity scale. Preserve unknown
    # issues for inspection rather than guessing that numeric values are errors.
    return [x for x in issues if not (x.get('type') == 2 and x.get('subtype') == 0
                                    and x.get('message') == 'Data stream: Time Mapping')]


def cpu_samples(path, pid, start_ns, end_ns):
    if not path.exists():
        return None
    root = ET.parse(path).getroot()
    refs = {e.get('id'): e for e in root.iter() if e.get('id')}
    def resolve(e):
        return refs[e.get('ref')] if e.get('ref') else e
    leafs = defaultdict(int); states = defaultdict(int); wait = total = 0
    for node in root.findall('node'):
        cols = [c.findtext('mnemonic') for c in node.findall('schema/col')]
        for row in node.findall('row'):
            d = {k: resolve(e) for k,e in zip(cols,row)}
            if not d['process'].get('fmt','').endswith(f'({pid})'):
                continue
            if 'Main Thread' not in d['thread'].get('fmt',''):
                continue
            if not start_ns <= int(d['time'].text) <= end_ns:
                continue
            stack = [resolve(f).get('name','') for f in d['stack'].findall('frame')]
            total += 1
            states[d['thread-state'].text] += 1
            leafs[stack[0] if stack else 'unknown'] += 1
            wait += any('waitUntilCompleted' in f for f in stack)
    return {'main_thread_samples_in_gpu_span': total, 'wait_until_completed_stacks': wait,
            'sampled_wait_fraction': wait/total if total else None, 'reported_thread_states': dict(states),
            'leaf_functions': sorted(leafs.items(),key=lambda x:-x[1]),
            'basis': 'stack sample counts within target GPU span, not exact CPU wall time; thread-state label alone does not identify blocking'}


def summarize(exports, pid):
    availability = json.loads((exports / 'availability.json').read_text())
    if trace_problems(availability['trace_issues']):
        raise ValueError('trace reports errors; refusing performance attribution')
    rows = [r for t in tables(exports / 'metal-gpu-intervals.xml') for r in t['rows']]
    all_target = [r for r in rows if r['process']['fmt'].endswith(f'({pid})')
                  and r['state']['raw'] == 'Active']
    target = [r for r in all_target if int(r['event-depth']['raw']) == 0]
    if not target:
        raise ValueError('no active GPU intervals for target pid')
    spans = [(int(r['start']['raw']), int(r['start']['raw']) + int(r['duration']['raw'])) for r in target]
    merged = intervals_union(spans)
    busy = sum(hi - lo for lo, hi in merged)
    total = merged[-1][1] - merged[0][0]
    duration = sorted(int(r['duration']['raw']) for r in target)
    groups = defaultdict(lambda: {'segments': 0, 'nanoseconds': 0, 'encoders': set(), 'thread_shapes': set()})
    run_groups = defaultdict(lambda: defaultdict(lambda: {'buffers': set(), 'spans': []}))
    for r in target:
        label = r['event-label']['fmt']
        run = re.search(r'run=(\d+) set=(\d+) first=(\w+)', label)
        if run:
            g = run_groups[(int(run[1]),int(run[2]))][run[3]]
            g['buffers'].add(r['cmdbuffer-id']['raw'])
            lo = int(r['start']['raw']);g['spans'].append((lo,lo+int(r['duration']['raw'])))
        names = list(dict.fromkeys(re.findall(r'fn=([\w_]+)', label)))
        name = '+'.join(names) if names else 'blit' if 'Blit' in label else 'unlabeled'
        g = groups[name]
        g['segments'] += 1
        g['nanoseconds'] += int(r['duration']['raw'])
        g['encoders'].add(r['encoder-id']['raw'])
        for shape in re.findall(r'fn=\w+ threads=\d+ indirect=(?:true|false)', label):
            g['thread_shapes'].add(shape)
    grouped = []
    for name, g in groups.items():
        grouped.append({'kernel': name, 'segments': g['segments'], 'encoders': len(g['encoders']),
                        'seconds': g['nanoseconds'] / 1e9, 'active_time_fraction': g['nanoseconds'] / busy,
                        'thread_shapes': sorted(g['thread_shapes'])})
    grouped.sort(key=lambda x: -x['seconds'])
    result = {'pid': pid, 'diagnostic_only': True, 'window_start_seconds': merged[0][0] / 1e9,
              'window_end_seconds': merged[-1][1] / 1e9, 'active_span_seconds': total / 1e9,
              'gpu_active_seconds': busy / 1e9, 'gpu_active_time_fraction': busy / total,
              'no_target_gpu_work_seconds': (total - busy) / 1e9,
              'overlapping_interval_seconds': (sum(duration) - busy) / 1e9,
              'segments': len(target), 'encoders': len({r['encoder-id']['raw'] for r in target}),
              'command_buffers': len({r['cmdbuffer-id']['raw'] for r in target}),
              'segments_under_10us': sum(d < 10000 for d in duration),
              'segment_p50_us': duration[len(duration) // 2] / 1000,
              'segment_p99_us': duration[min(len(duration) - 1, int(len(duration) * .99))] / 1000,
              'encoder_groups': grouped, 'nested_gpu_segments_excluded': len(all_target)-len(target),
              'available_rows': availability['rows'],
              'limits': ['Time-active fraction is not ALU occupancy, bandwidth saturation or matrix utilization.',
                         'Encoder segments can be split by preemption; their durations are summed, not their end-to-end envelope.',
                         'Profiling changes timing. Use an uninstrumented verified control for speedups.',
                         'Run metadata equality does not establish full graph or native semantic equality.']}
    shaders = defaultdict(lambda: [0, 0])
    spans = []
    for t in tables(exports / 'metal-shader-profiler-intervals.xml'):
        for r in t['rows']:
            if not r['process']['fmt'].endswith(f'({pid})'):
                continue
            name = re.sub(r' \(\d+\)$', '', r['name']['fmt'])
            dt = int(r['duration']['raw']); lo = int(r['start']['raw'])
            shaders[name][0] += dt; shaders[name][1] += 1
            spans.append((lo, lo+dt))
    shader_total = sum(x[0] for x in shaders.values())
    result['shader_hotspots'] = [{'kernel': n, 'sampled_seconds': v[0]/1e9, 'intervals': v[1],
                                 'sampled_time_share': v[0]/shader_total}
                                for n, v in sorted(shaders.items(), key=lambda x: -x[1][0])]
    result['shader_time_basis'] = 'Instruments sampled shader timeline; encoder groups may fuse multiple kernels'
    result['sampled_shader_interval_union_seconds'] = sum(b-a for a,b in intervals_union(spans))/1e9
    result['per_run'] = [{'run': run, 'ruleset_index': rs,
                         'stages': {n: {'command_buffers': len(g['buffers']),
                                        'gpu_active_seconds': sum(b-a for a,b in intervals_union(g['spans']))/1e9}
                                    for n,g in stages.items()}}
                        for (run,rs),stages in sorted(run_groups.items())]
    result['cpu_samples'] = cpu_samples(exports/'time-profile.xml',pid,merged[0][0],merged[-1][1])
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--exports', type=Path, required=True)
    p.add_argument('--replay', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    replay = json.loads(a.replay.read_text())
    if not replay['saved_run_metadata_equal']:
        raise ValueError('replay check failed')
    data = summarize(a.exports, replay['worker_pid'])
    a.output.write_text(json.dumps(data, indent=2) + '\n')
    print(json.dumps({k: v for k, v in data.items() if k not in ['limits', 'available_rows']}, indent=2))


if __name__ == '__main__':
    main()

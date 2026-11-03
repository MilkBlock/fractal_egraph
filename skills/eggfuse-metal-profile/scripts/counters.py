#!/usr/bin/env python3
"""Stream hardware counters and correlate with named GPU-active intervals.

Counters are device-wide. Correlation is not per-process isolation, and time
averages of vendor percentages do not become a proof of bottleneck causality.
"""
import argparse
from bisect import bisect_right
from collections import defaultdict
import json
import math
from pathlib import Path
import re
import xml.etree.ElementTree as ET
from report import tables, intervals_union, trace_problems


class Coverage:
    def __init__(self, intervals):
        self.spans = intervals_union(intervals)
        self.starts = [x[0] for x in self.spans]
        self.prefix = [0]
        for lo, hi in self.spans:
            self.prefix.append(self.prefix[-1] + hi - lo)

    def integral(self, x):
        i = bisect_right(self.starts, x) - 1
        if i < 0:
            return 0
        lo, hi = self.spans[i]
        return self.prefix[i] + min(x, hi) - lo

    def overlap(self, lo, hi):
        return self.integral(hi) - self.integral(lo)


def attribution_context(gpu_path,pid):
    """Expose background overlap and missing target shader metadata explicitly."""
    by_process=defaultdict(list)
    for t in tables(gpu_path):
        for r in t['rows']:
            if r['state']['raw']!='Active' or int(r['event-depth']['raw'])!=0:continue
            lo=int(r['start']['raw']);by_process[r['process']['fmt']].append((lo,lo+int(r['duration']['raw'])))
    own=[span for name,spans in by_process.items() if name.endswith(f'({pid})') for span in spans]
    coverage=Coverage(own);total=coverage.prefix[-1]
    others={name:spans for name,spans in by_process.items() if not name.endswith(f'({pid})')}
    merged=intervals_union([span for spans in others.values() for span in spans])
    overlap=sum(coverage.overlap(a,b) for a,b in merged)
    shader_rows=[r for t in tables(gpu_path.with_name('metal-shader-profiler-intervals.xml')) for r in t['rows'] if r['process']['fmt'].endswith(f'({pid})')]
    registered=[r for t in tables(gpu_path.with_name('metal-shader-profiler-shader-list.xml')) for r in t['rows'] if r['process']['fmt'].endswith(f'({pid})')]
    shader_spans=intervals_union([(int(r['start']['raw']),int(r['start']['raw'])+int(r['duration']['raw'])) for r in shader_rows])
    shader_covered=sum(coverage.overlap(a,b) for a,b in shader_spans)
    return dict(target_shader_sample_intervals=len(shader_rows),target_registered_shaders=len(registered),per_shader_attribution_available=bool(shader_rows),
        target_shader_sample_covered_seconds=shader_covered/1e9,target_shader_sample_coverage_fraction=shader_covered/total if total else None,
        other_process_overlap_seconds=overlap/1e9,other_process_overlap_fraction=overlap/total if total else None,
        other_processes=[dict(process=name,overlap_seconds=sum(coverage.overlap(a,b) for a,b in intervals_union(spans))/1e9) for name,spans in others.items() if sum(coverage.overlap(a,b) for a,b in intervals_union(spans))],
        interpretation='Device-wide measurements overlapping target work; not exclusive kernel occupancy/bandwidth. Missing target shader samples remain unavailable.')


def summarize(path, gpu_path, pid, top=6):
    groups = defaultdict(list)
    devices = set()
    for t in tables(gpu_path):
        for r in t['rows']:
            if not r['process']['fmt'].endswith(f'({pid})') or r['state']['raw'] != 'Active':
                continue
            if int(r['event-depth']['raw']) != 0:
                continue
            if r.get('gpu'):
                devices.add(r['gpu']['fmt'])
            lo = int(r['start']['raw']); hi = lo + int(r['duration']['raw'])
            groups['all_target_active'].append((lo, hi))
    for t in tables(gpu_path.with_name('metal-shader-profiler-intervals.xml')):
        for r in t['rows']:
            if r['process']['fmt'].endswith(f'({pid})'):
                name = re.sub(r' \(\d+\)$', '', r['name']['fmt'])
                lo = int(r['start']['raw']); hi = lo+int(r['duration']['raw'])
                groups[name].append((lo, hi))
    if not groups:
        raise ValueError('target has no GPU interval')
    if len(devices) > 1:
        raise ValueError('multi-device target requires per-device counter correlation')
    names = ['all_target_active'] + sorted((x for x in groups if x != 'all_target_active'),
                key=lambda x: -sum(b-a for a, b in groups[x]))[:top]
    coverage = {name: Coverage(groups[name]) for name in names}
    refs = {}
    wanted = {'start-time', 'duration', 'uint32', 'gpu-counter-name', 'fixed-decimal',
              'percent', 'boolean', 'metal-device-name'}
    agg = defaultdict(lambda: defaultdict(lambda: [0., 0., float('-inf'), 0]))
    units = {}; samples = 0; cache = {}; cols = None
    node = None
    def value(e):
        return refs[e.get('ref')] if e.get('ref') else (e.text or '', e.get('fmt', e.text or ''))
    for event, e in ET.iterparse(path, events=('start', 'end')):
        if event == 'start':
            if e.tag == 'node':node = e
            continue
        if e.get('id') and e.tag in wanted:
            refs[e.get('id')] = (e.text or '', e.get('fmt', e.text or ''))
        if e.tag == 'schema':
            cols = [c.findtext('mnemonic') for c in e.findall('col')]
        if e.tag != 'row':
            continue
        row = dict(zip(cols, e))
        if devices and 'gpu' in row and value(row['gpu'])[1] not in devices:
            e.clear()
            if node is not None:node.remove(e)
            continue
        lo = int(value(row['start'])[0]); duration = int(value(row['duration'])[0])
        weights = cache.get((lo, duration))
        if weights is None:
            weights = [(name, c.overlap(lo, lo+duration)) for name, c in coverage.items()]
            cache[lo, duration] = weights
        name = value(row['name'])[1]
        v = float(value(row['value'])[0])
        if not math.isfinite(v):
            raise ValueError('non-finite hardware counter value')
        units.setdefault(name, {'percentage': value(row['is-percentage'])[0] == '1',
                                'sample_format': value(row['value'])[1],
                                'description': row['label'].get('fmt')})
        for group, weight in weights:
            if weight:
                a = agg[group][name]; a[0] += v*weight; a[1] += weight
                a[2] = max(a[2], v); a[3] += 1
        samples += 1
        e.clear()
        if node is not None:
            node.remove(e)
    if not samples or not agg['all_target_active']:
        raise ValueError('hardware counters are empty or do not overlap target execution')
    return {'device_wide_counters': True, 'attribution': attribution_context(gpu_path,pid), 'rows': samples, 'unique_time_intervals': len(cache),
            'scope': 'time-weighted samples overlapping target GPU intervals or sampled shader timeline; other processes may contribute',
            'counter_metadata': units,
            'groups': {g: {'active_seconds': coverage[g].prefix[-1]/1e9,
                        'counters': {n: {'mean': a[0]/a[1], 'peak': a[2], 'samples': a[3],
                                        'overlap_seconds': a[1]/1e9}
                                     for n, a in values.items() if a[1]}}
                       for g, values in agg.items()}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--exports', type=Path, required=True)
    p.add_argument('--replay', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a=p.parse_args()
    availability=json.loads((a.exports/'availability.json').read_text())
    if trace_problems(availability['trace_issues']):
        raise ValueError('trace reports errors')
    pid=json.loads(a.replay.read_text())['worker_pid']
    result=summarize(a.exports/'metal-gpu-counter-intervals.xml', a.exports/'metal-gpu-intervals.xml', pid)
    a.output.write_text(json.dumps(result,indent=2)+'\n')
    print('summarized', result['rows'], 'counter samples;',len(result['groups']),'time-correlated groups')


if __name__ == '__main__':
    main()

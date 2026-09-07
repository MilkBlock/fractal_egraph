#!/usr/bin/env python3
"""Direct engine baseline. Each run constructs its own graph; no replay or templates."""
import argparse, datetime, hashlib, json, platform, random, re, statistics, subprocess
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'results/slotted_baseline'


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--repeats', type=int, default=3)
    args = p.parse_args()
    assert args.repeats > 0
    OUT.mkdir(parents=True, exist_ok=True)
    binary = ROOT / 'target/release/slotted_baseline'
    meta = {
        'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'platform': platform.platform(),
        'cpu': subprocess.check_output(['sysctl', '-n', 'machdep.cpu.brand_string'], text=True).strip(),
        'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
        'slotted_version': '0.0.36', 'slotted_features': [],
        'slotted_git_revision': 'ddda46116edfb3170d8b9caca08527deebc4854f',
        'egglog_upstream': 'ebba7bb902bdc1b0f377b6bb22c06ac305912674',
        'egglog_trace': False, 'profile': 'release', 'repetitions': args.repeats,
        'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
        'cargo_lock_sha256': hashlib.sha256((ROOT/'Cargo.lock').read_bytes()).hexdigest(),
    }
    rows = []
    for case, sizes in [('renamed', [1, 64, 1024]), ('constants', [1, 64, 1024]), ('ac', [1, 16, 128])]:
        for count in sizes:
            for repeat in range(args.repeats):
                modes = ['egglog', 'slotted']
                random.Random(9021 + repeat).shuffle(modes)
                for mode in modes:
                    r = subprocess.run(['/usr/bin/time', '-l', str(binary), mode, case, str(count)],
                                       cwd=ROOT, text=True, capture_output=True, timeout=60, check=True)
                    x = json.loads(r.stdout)
                    assert not x['slotted_checks'], 'benchmark must use release without checks'
                    assert x['saturated'] and x['verified_terms'] > 0
                    m = re.search(r'(\d+)\s+maximum resident set size', r.stderr)
                    x.update(repeat=repeat, peak_rss_bytes=int(m.group(1)) if m else None)
                    rows.append(x)
                    print(case, count, mode, 'nodes=', x['constructor_nodes'],
                          'KiB=', round(x['retained_requested_bytes']/1024, 2),
                          'rewrite_ms=', round(x['rewrite_ns']/1e6, 2), flush=True)
                    (OUT/'runs.json').write_text(json.dumps({'environment': meta, 'runs': rows}, indent=2)+'\n')
    summary = []
    for case, count, mode in sorted({(x['case'], x['roots'], x['engine']) for x in rows}):
        rr = [x for x in rows if (x['case'],x['roots'],x['engine']) == (case,count,mode)]
        fields = ['constructor_nodes', 'live_classes', 'distinct_root_classes', 'external_slot_map_entries',
                  'retained_requested_bytes','peak_requested_bytes','peak_rss_bytes','build_ns','rewrite_ns','rounds','verified_terms']
        x = {k: statistics.median(y[k] for y in rr) for k in fields}
        x.update(case=case, roots=count, engine=mode,
                 build_min_ns=min(y['build_ns'] for y in rr), build_max_ns=max(y['build_ns'] for y in rr),
                 rewrite_min_ns=min(y['rewrite_ns'] for y in rr), rewrite_max_ns=max(y['rewrite_ns'] for y in rr))
        summary.append(x)
    (OUT/'summary.json').write_text(json.dumps({'environment':meta, 'aggregate':summary},indent=2)+'\n')
    lines = ['# Slotted e-graph baseline', '',
             'Pinned slotted-egraphs 0.0.36 versus the local egglog engine (trace disabled). Direct graph construction and rewrite execution. No SharedStore, no snapshot replay, no dictionary-compression ratios.', '',
             '| case | roots | engine | nodes | live classes | root classes | retained KiB | peak RSS MiB | build ms | rewrite ms | rounds |',
             '|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|']
    for x in summary:
        lines.append(f"| {x['case']} | {x['roots']} | {x['engine']} | {x['constructor_nodes']} | {x['live_classes']} | {x['distinct_root_classes']} | {x['retained_requested_bytes']/1024:.2f} | {x['peak_rss_bytes']/1048576:.2f} | {x['build_ns']/1e6:.3f} | {x['rewrite_ns']/1e6:.3f} | {x['rounds']} |")
    lines += ['', '## Interpretation and limits', '',
              '- renamed: Add(Mul(a,b),Neg(a)); distinct free-variable names per input, with sharing of a across branches. constants uses the same shape with distinct numeric constants. ac starts from (a+b)+(c+d), applies add commutativity and both associativity directions to saturation.',
              '- All external roots remain live. Slotted root handles contain their actual SlotMaps, and these allocations are included. Equal class IDs alone are never accepted as semantic equality; AppliedId mappings are checked.',
              '- Constructor-node counts exclude egglog primitive i64 records but include Var/Num constructors. All engine allocations, indexes, union-find, caches, rules and root handles remain included in requested-byte measurements. Different implementations have different metadata costs.',
              '- Retained/peak requested bytes are tracked with a System allocator wrapper, relative to the process argument baseline. They exclude allocator metadata and memory outside Rust System allocations. RSS is the whole process maximum, including subsequent correctness/statistics queries.',
              '- Build time includes initialization, input generation, parsing and insertion via each public API. egglog also resolves/types expressions. It is an end-to-end frontend measurement, not an isolated data-structure operation comparison. No internal let function is created per root.',
              '- Rewrite time includes each engine\'s actual matching, application and rebuilding until its own change/progress detector reports saturation (cap 32 rounds, failure rather than partial success). These engines need not use the same number of rounds.',
              '- Correctness is checked after memory/time snapshots using read-only term lookup. AC verifies all 120 four-leaf binary trees/permutations for up to three input roots per measured run, plus a negative cross-root check. This is a scoped AC equivalence check, not general equivalence of all engine behavior.',
              '- Unit tests with slotted internal checks enabled also verify cross-layer repeated-variable wiring, constant separation and binder alpha-equivalence without variable capture.',
              '- These are three controlled workloads, not production compiler workloads. Small absolute RSS differences may be process noise; use requested bytes, node counts and scaling together. Slotted is not assumed to win for constants or every workload.',
              '', '## Reproduce', '', '```sh',
              'cargo test --bin slotted_baseline --features slotted-checks',
              'cargo build --release --bin slotted_baseline',
              'python3 experiments/slotted_baseline/run.py', '```', '',
              'Source: https://github.com/memoryleak47/slotted-egraphs/ ; version and transitive checksums are pinned in Cargo.lock.']
    (OUT/'report.md').write_text('\n'.join(lines)+'\n')


if __name__ == '__main__':
    main()

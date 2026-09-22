#!/usr/bin/env python3
"""Probe every served example against the pinned kernel.

The load-an-example list is a build artifact of ../egglog-demo, which tracks upstream egglog
main, while this repository pins the pristine egg-smol baseline plus local instrumentation.
Examples that use newer syntax therefore appear in the list but can never run here -- the egg
math demo works, math-backoff fails to parse at `(let-scheduler bo (back-off))`.

This records, per example, whether the current binary can parse it. The server hides the ones
that cannot, so the page stops offering entries that are guaranteed to fail.
"""
import argparse
import json
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from server import ROOT, merged_examples, DEFAULT_DEMO  # noqa: E402

OUT = Path(__file__).resolve().parent / 'example-support.json'


def probe(binary, name, source, folder):
    """Parse one example with the real binary. Returns (supported, detail)."""
    path = folder / f'{abs(hash(name)):x}.egg'
    path.write_text(source)
    result = subprocess.run([str(binary), 'parse-check', str(path)],
                            capture_output=True, text=True, cwd=ROOT)
    if result.returncode == 0:
        return True, ''
    tail = (result.stderr or result.stdout).strip().splitlines()
    return False, (tail[-1][:200] if tail else f'exit {result.returncode}')


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/egg_layout')
    parser.add_argument('--demo', type=Path, default=DEFAULT_DEMO)
    parser.add_argument('--output', type=Path, default=OUT)
    args = parser.parse_args()

    examples = merged_examples(args.demo)
    supported, unsupported = [], {}
    with tempfile.TemporaryDirectory(prefix='probe-examples-') as tmp:
        folder = Path(tmp)
        for name in sorted(examples):
            ok, detail = probe(args.binary, name, examples[name], folder)
            if ok:
                supported.append(name)
            else:
                unsupported[name] = detail
            print(f"  {'ok  ' if ok else 'FAIL'} {name}" + ('' if ok else f'  <- {detail[:90]}'))

    payload = {
        'scope': ('Parse check with the pinned kernel only. Parsing is necessary, not '
                  'sufficient: it says nothing about whether a run will saturate.'),
        'binary': str(args.binary.relative_to(ROOT)) if args.binary.is_relative_to(ROOT) else str(args.binary),
        'probed_at': datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
        'total': len(examples),
        'supported': supported,
        'unsupported': unsupported,
    }
    args.output.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + '\n')
    print(f"\n{len(supported)}/{len(examples)} 可解析 -> {args.output}")
    return 0


if __name__ == '__main__':
    sys.exit(main())

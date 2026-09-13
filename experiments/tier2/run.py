"""One pipeline driver; native operations share the egg_layout executable."""
import argparse
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

def options(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--driver', type=Path, help='already built egg_layout executable')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--recapture-tier0', action='store_true', help='capture Math tier-0 again before analysis')
    mode.add_argument('--reuse-tier0', action='store_true', help='reuse the committed snapshot or --output directory')
    parser.add_argument('--source', type=Path, help='self-contained Math .egg input; requires --recapture-tier0')
    parser.add_argument('--rounds', type=int, help='override one simple (run N); omitted: preserve the source schedule')
    parser.add_argument('--output', type=Path, help='new capture directory, or an existing completed run to reuse')
    parser.add_argument('--skip-checks', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if args.rounds is not None and (not args.recapture_tier0 or args.rounds < 1):
        parser.error('--rounds must be positive and requires --recapture-tier0')
    if args.source is not None and not args.recapture_tier0:
        parser.error('--source requires --recapture-tier0')
    return args

def main():
    args = options()
    driver = args.driver.resolve() if args.driver else ROOT / 'target/debug/egg_layout'
    os.chdir(ROOT)

    def run(*command):
        subprocess.run(command, check=True)

    def py(script):
        run(sys.executable, f'experiments/tier2/{script}.py')

    def native(command, *arguments):
        run(str(driver), command, *arguments)

    if args.driver is None:
        run('cargo', 'build', '--quiet', '--bin', 'egg_layout')

    if args.recapture_tier0 or args.output:
        from capture import capture_or_reuse
        capture_or_reuse(ROOT, driver, args.output, args.rounds, args.recapture_tier0, args.source)
        return

    py('mine')
    native('relations', 'experiments/tier2/math.egg', 'experiments/tier2/math_native.json')
    py('higher')
    native('higher')
    py('check_higher')

    # The recurrence fixtures are a separate adapter, not the generic Math learner.
    py('fixtures')
    for name in ['unit', 'triple', 'stride', 'changing', 'warmup']:
        source = Path('experiments/tier2/fixtures') / f'{name}.egg'
        native('fixture', str(source), str(source.with_suffix('.json')))
        native('schema', str(source), str(source.with_suffix('.ast.json')))
    py('recurrence_bridge')
    native('observations')
    py('affine')
    native('relations', 'experiments/tier2/affine.egg', 'experiments/tier2/affine_native.json')
    native('reduce')
    py('render')
    py('fractal_view')

    if args.skip_checks:
        return
    run(sys.executable, '-m', 'unittest', 'discover', '-s', 'experiments/tier2', '-p', 'test_*.py')
    run('cargo', 'test', '--quiet', '--test', 'tier2_native', '--test', 'tier2_reduce')

if __name__ == '__main__':
    main()

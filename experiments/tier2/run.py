"""One pipeline driver; native operations share the egg_layout executable."""
import argparse
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--driver', type=Path, help='already built egg_layout executable')
    args = parser.parse_args()
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

    run(sys.executable, '-m', 'unittest', 'discover', '-s', 'experiments/tier2', '-p', 'test_*.py')
    run('cargo', 'test', '--quiet', '--test', 'tier2_native', '--test', 'tier2_reduce')

if __name__ == '__main__':
    main()

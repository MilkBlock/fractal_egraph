"""Stable analyzer entry, retaining the native Rust CLI during crate migration."""
import json,subprocess
from pathlib import Path
from .recipes import adapt

ROOT=Path(__file__).resolve().parents[2]

def publish_recipes(directory):
    directory=Path(directory)
    from .health import publish as publish_health
    publish_health(directory)
    source=directory/'cell-device-recipes.json'
    raw=json.loads(source.read_text())
    from eggfuse.ir.structural_key import validate_core
    for p in raw['programs']:validate_core(p)
    if raw.get('source_program',{}).get('status')=='ready':validate_core(raw['source_program']['core'])
    recipe=adapt(raw)
    target=directory/'recipe-library.json';target.write_text(json.dumps(recipe,indent=2)+'\n')
    source_program=recipe.get('source_program',{})
    from .merge_programs import build as merge_programs
    merge_library={'schema':'eggfuse-merge-recipe-library/v1',
        'source':merge_programs(source_program['recipe']) if source_program.get('status')=='ready' else None,
        'programs':[merge_programs(p) for p in recipe['programs']],
        'terminal_cell_substitution_authorized':False}
    (directory/'parameterized-merge-programs.json').write_text(json.dumps(merge_library,indent=2)+'\n')
    if source_program.get('status')=='ready':
        from .regions import analyze
        (directory/'region-program.json').write_text(json.dumps(analyze(source_program['recipe']),indent=2)+'\n')
    return target

def main(args,binary=None):
    binary=Path(binary) if binary else ROOT/'target/debug/egg_layout'
    if not binary.is_file():raise ValueError('build the native analyzer first: cargo build --offline --bin egg_layout')
    subprocess.run([str(binary.resolve()),*(['--help'] if args in (['--help'],['-h']) else ['analyze',*args])],check=True)
    if '--output' in args:
        output=Path(args[args.index('--output')+1]);print('Recipe library:',publish_recipes(output))

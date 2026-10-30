"""Canonical project entry; native analyzer CLI remains valid."""
import argparse,sys

def main():
    parser=argparse.ArgumentParser(prog='eggfuse',description='eggfuse analyzer, compiler and engine')
    parser.add_argument('command',choices=['analyze','compile','run','run-cells','replay','report','inspect-recipe','cpu'])
    if len(sys.argv)==1:parser.print_help();return 0
    if sys.argv[1] in ['-h','--help']:parser.print_help();return 0
    command=parser.parse_args(sys.argv[1:2]).command;args=sys.argv[2:]
    if command=='cpu':
        from .engine.cpu import main as cpu
        return cpu(args)
    if command=='replay':
        from .engine.replay import main as replay
        return replay(args)
    if command=='analyze':
        from .analyzer.native import main as analyze
        p=argparse.ArgumentParser(add_help=False);p.add_argument('--analyzer-bin');a,rest=p.parse_known_args(args)
        analyze(rest,a.analyzer_bin);return 0
    if command in ['run','run-cells']:
        from .engine.metal import main as run
        return run(args,library=command=='run-cells')
    if command=='inspect-recipe':
        import json
        from pathlib import Path
        from .ir.recipe import validate
        p=argparse.ArgumentParser(prog='eggfuse inspect-recipe');p.add_argument('path',type=Path);a=p.parse_args(args)
        recipe=json.loads(a.path.read_text());validate(recipe)
        print(json.dumps(dict(schema=recipe['schema'],scope=recipe['scope'],rules=len(recipe['rules']),rulesets=len(recipe['rulesets']),event_routes=len(recipe['event_routes']),capabilities=recipe['capabilities'],authorization=recipe['authorization']),indent=2));return 0
    sys.argv=['eggfuse '+command,*args]
    if command=='compile':
        from .compiler.pipeline import main as compile
        compile()
    else:
        from .compiler.report import main as report
        report()
    return 0

if __name__=='__main__':raise SystemExit(main())

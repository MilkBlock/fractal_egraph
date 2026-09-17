#!/usr/bin/env python3
"""Answer annotation queries with egglog-demo's preview_annotations.py.

The browser bundle reimplements that module so the published page needs no
bridge; this reference keeps the two honest by answering the same questions.
Requests arrive as JSON on stdin, results leave as JSON on stdout.
"""
import argparse
import importlib.util
import json
import sys


def load_module(demo):
    spec = importlib.util.spec_from_file_location('preview_annotations', f'{demo}/preview_annotations.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def answer(module, request):
    op = request['op']
    if op == 'rewrite_preview_entry':
        return module.rewrite_preview_entry(request['source'], request['line'])
    if op == 'preview_source':
        return module.preview_source(request['source'], request['line'])
    if op == 'catalog':
        return module.catalog(request['source'], request['line'])
    if op == 'update_conditions':
        return module.update_conditions(request['source'], request['line'], request['conditions'])
    if op == 'update_display':
        return module.update_display(request['source'], request['line'], request['target_id'],
                                    request['value'], request.get('fields'),
                                    request.get('template'), request.get('precedence'))
    raise SystemExit(f'unknown op {op}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--demo', required=True)
    args = parser.parse_args()
    module = load_module(args.demo)
    results = []
    for request in json.load(sys.stdin):
        try:
            results.append({'ok': answer(module, request)})
        except Exception as error:  # ValueError and friends are part of the contract
            results.append({'error': str(error)})
    json.dump(results, sys.stdout, ensure_ascii=False)


if __name__ == '__main__':
    main()

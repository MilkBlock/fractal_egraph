"""Lossless push/pop scope extraction using egglog's native parser.

Output state is the selected scope's state BEFORE pop, with its inherited
prefix replayed. This is source normalization, not saturation or Cell Bake.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def native_ast(source, folder):
    dest = folder / 'ast.json'
    subprocess.run([ROOT/'target/release/examples/split_ast', source, dest], check=True, cwd=ROOT)
    return json.loads(dest.read_text())


def plan(source, data):
    commands = data['commands']
    for command in commands:
        if command['kind'] == 'unsupported':
            raise ValueError(f"line {command['line']}: scope splitting does not support {command.get('command')}; original file was not changed")
    scopes = []
    root = dict(id=0, line=1, prefix=[], local=[])
    stack = [root]
    next_id = 1
    for ordinal, command in enumerate(commands):
        kind = command['kind']
        if kind == 'push':
            for _ in range(command['count']):
                parent = stack[-1]
                stack.append(dict(id=next_id, line=command['line'],
                                  prefix=parent['prefix']+parent['local'], local=[]))
                next_id += 1
        elif kind == 'pop':
            if command['count'] >= len(stack):
                raise ValueError(f"line {command['line']}: pop exceeds the active push stack")
            for _ in range(command['count']):
                frame = stack.pop()
                frame['end_line'] = command['line']
                scopes.append(frame)
        else:
            stack[-1]['local'].append(ordinal)
    if len(stack) != 1:
        raise ValueError(f"unclosed push at line {stack[-1]['line']}; no files written")
    root['end_line'] = len(source.read_text().splitlines())
    scopes.append(root)
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    files, skipped = [], []
    for scope in sorted(scopes, key=lambda s: s['id']):
        # An inherited run by itself is not a new example. Direct runs belong
        # to exactly one scope; completed nested scopes roll back and are absent.
        if not any(commands[i]['kind'] == 'schedule' for i in scope['local']):
            continue
        history = scope['prefix'] + scope['local']
        names = sorted({r for i in history for r in commands[i].get('rulesets', [])})
        location = dict(scope=scope['id'], line=scope['line'], end_line=scope['end_line'])
        if len(names) != 1:
            skipped.append(dict(location, rulesets=names, reason='multiple_scheduled_rulesets' if names else 'no_scheduled_ruleset',
                                schedule_lines=[commands[i]['line'] for i in history if commands[i]['kind']=='schedule']))
            continue
        ruleset = names[0]
        kept = [i for i in history if
                (commands[i]['kind'] != 'rule' or commands[i]['ruleset'] == ruleset) and
                (commands[i]['kind'] != 'ruleset' or commands[i]['name'] == ruleset)]
        kept_set = set(kept)
        removed = [i for i in history if i not in kept_set]
        name = re.sub(r'[^A-Za-z0-9_-]', '_', ruleset) or 'default'
        dest = source.with_name(f'{source.stem}.split-{scope["id"]:03d}-{name}.egg')
        info = dict(location, ruleset=ruleset, path=str(dest), source_ordinals=kept,
                    dropped_unrun_rule_ordinals=removed,
                    inherited_schedule_lines=[commands[i]['line'] for i in scope['prefix'] if commands[i]['kind']=='schedule'])
        header = '; @egg-fractal-split ' + json.dumps(dict(source=source.name, sha256=digest, **location), ensure_ascii=False) + '\n'
        body = ''.join(commands[i]['leading'] + commands[i]['text'] for i in kept) + '\n'
        # Statements and their adjacent comments are copied verbatim; never use
        # normalized AST printing (e.g. `_` -> reserved `@_`).
        files.append(dict(info, content=header+body))
    return dict(schema='egglog-scope-split/v1', source=str(source), source_sha256=digest,
                semantics='selected scope before pop; inherited prefix replayed; schedules and checks unchanged',
                normalization_only=True, files=files, skipped=skipped)


def publish(report):
    source = Path(report['source'])
    manifest = source.with_name(source.stem+'.split.json')
    public = {**report, 'manifest': str(manifest),
              'files': [{k:v for k,v in f.items() if k != 'content'} for f in report['files']]}
    writes = [(Path(f['path']), f['content'].encode()) for f in report['files']]
    writes.append((manifest, (json.dumps(public, ensure_ascii=False, indent=2)+'\n').encode()))
    # Validate the entire write set first: edited outputs are never overwritten,
    # and identical reruns reuse existing files rather than multiplying them.
    for file, content in writes:
        if file.exists() and (not file.is_file() or file.read_bytes() != content):
            raise ValueError(f'output already exists with different content: {file}; move it or choose another source filename')
    if hashlib.sha256(source.read_bytes()).hexdigest() != report['source_sha256']:
        raise ValueError('source changed while splitting; no files written')
    created = []
    try:
        for file, content in writes:
            try:
                with file.open('xb') as stream:
                    created.append((file, content))
                    stream.write(content)
            except FileExistsError:
                if file.read_bytes() != content:
                    raise ValueError(f'output changed while splitting: {file}')
    except Exception:
        for file, content in reversed(created):
            if file.is_file() and file.read_bytes() == content:
                file.unlink()
        raise
    return public


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source', type=Path, required=True)
    p.add_argument('--skip-build', action='store_true')
    p.add_argument('--dry-run', action='store_true')
    args = p.parse_args()
    # Keep a symlinked input's visible directory: outputs belong beside the
    # file the user selected, not beside its resolved target in another tree.
    source = args.source.absolute()
    if source.suffix.lower() != '.egg':
        raise ValueError('source must be a .egg file')
    if not args.skip_build:
        subprocess.run(['cargo','build','--offline','--release','--example','split_ast'], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix='egg-split-') as temp:
        folder = Path(temp)
        before = hashlib.sha256(source.read_bytes()).hexdigest()
        report = plan(source, native_ast(source, folder))
        if before != report['source_sha256']:
            raise ValueError('source changed during parsing')
        for file in report['files']:
            check = folder/'output.egg'
            check.write_text(file['content'])
            parsed = native_ast(check, folder)
            rulesets = {c['ruleset'] for c in parsed['commands'] if c['kind']=='rule'}
            if rulesets - {file['ruleset']}:
                raise AssertionError('split output contains an unscheduled ruleset')
        public = publish(report) if not args.dry_run else {**report, 'files':[
            {k:v for k,v in f.items() if k!='content'} for f in report['files']]}
    print(json.dumps(public, ensure_ascii=False))


if __name__ == '__main__':
    main()

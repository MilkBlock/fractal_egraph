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
    output_stem = source.stem
    base_manifest = source.with_name(source.stem+'.split.json')
    if base_manifest.is_file():
        try:
            previous = json.loads(base_manifest.read_text())
        except (ValueError, UnicodeError):
            previous = {}
        if (previous.get('schema') in ('egglog-scope-split/v1', 'egglog-scope-split/v2')
                and previous.get('source') == str(source)
                and re.fullmatch('[0-9a-f]{64}', previous.get('source_sha256', ''))
                and previous['source_sha256'] != digest):
            # Source edits start a separate deterministic batch beside the
            # original, preserving all earlier generated or user-edited files.
            output_stem += '.'+digest[:12]
    files = []
    for scope in sorted(scopes, key=lambda s: s['id']):
        # An inherited run by itself is not a new example. Direct runs belong
        # to exactly one scope; completed nested scopes roll back and are absent.
        if not any(commands[i]['kind'] == 'schedule' for i in scope['local']):
            continue
        history = scope['prefix'] + scope['local']
        names = sorted({r for i in history for r in commands[i].get('rulesets', [])})
        location = dict(scope=scope['id'], line=scope['line'], end_line=scope['end_line'])
        ruleset = names[0] if len(names) == 1 else None
        kept = [i for i in history if
                (commands[i]['kind'] != 'rule' or commands[i]['ruleset'] in names) and
                (commands[i]['kind'] != 'ruleset' or commands[i]['name'] in names)]
        kept_set = set(kept)
        removed = [i for i in history if i not in kept_set]
        name = (re.sub(r'[^A-Za-z0-9_-]', '_', ruleset) or 'default') if ruleset is not None else ('multi' if names else 'no-rules')
        dest = source.with_name(f'{output_stem}.split-{scope["id"]:03d}-{name}.egg')
        info = dict(location, ruleset=ruleset, path=str(dest), source_ordinals=kept,
                    dropped_unrun_rule_ordinals=removed,
                    inherited_schedule_lines=[commands[i]['line'] for i in scope['prefix'] if commands[i]['kind']=='schedule'])
        info.update(rulesets=names,
                    schedule_lines=[commands[i]['line'] for i in history if commands[i]['kind']=='schedule'],
                    bake={'status': 'single_ruleset' if len(names)==1 else ('requires_multi_ruleset_support' if names else 'no_scheduled_ruleset'),
                          'validation': 'static ruleset check only; successful Bake still requires compiler validation'})
        header = '; @egg-fractal-split ' + json.dumps(dict(source=source.name, sha256=digest, **location), ensure_ascii=False) + '\n'
        body = ''.join(commands[i]['leading'] + commands[i]['text'] for i in kept) + '\n'
        # Statements and their adjacent comments are copied verbatim; never use
        # normalized AST printing (e.g. `_` -> reserved `@_`).
        files.append(dict(info, content=header+body))
    return dict(schema='egglog-scope-split/v2', source=str(source), source_sha256=digest,
                semantics='selected scope before pop; inherited prefix replayed; schedules and checks unchanged',
                normalization_only=True, files=files, skipped=[],
                manifest=str(source.with_name(output_stem+'.split.json')))


def legacy_manifest(report, manifest):
    """Recognize exactly the v1 manifest this source would previously produce.

    This permits migration without overwriting arbitrary or edited metadata.
    Existing .egg files still must match byte-for-byte, with no overwrite.
    """
    fields = ('scope','line','end_line','ruleset','path','source_ordinals',
              'dropped_unrun_rule_ordinals','inherited_schedule_lines')
    files = [{k:f[k] for k in fields} for f in report['files'] if len(f['rulesets'])==1]
    skipped = [dict(scope=f['scope'], line=f['line'], end_line=f['end_line'], rulesets=f['rulesets'],
                    reason='multiple_scheduled_rulesets' if f['rulesets'] else 'no_scheduled_ruleset',
                    schedule_lines=f['schedule_lines']) for f in report['files'] if len(f['rulesets'])!=1]
    return {**report, 'schema':'egglog-scope-split/v1', 'files':files, 'skipped':skipped, 'manifest':str(manifest)}


def encoded(value):
    return (json.dumps(value, ensure_ascii=False, indent=2)+'\n').encode()


def publish(report):
    source = Path(report['source'])
    manifest = Path(report['manifest'])
    public = {**report, 'manifest': str(manifest),
              'files': [{k:v for k,v in f.items() if k != 'content'} for f in report['files']]}
    writes = [(Path(f['path']), f['content'].encode()) for f in report['files']]
    writes.append((manifest, encoded(public)))
    previous = encoded(legacy_manifest(report, manifest))
    # Validate the entire write set first: edited outputs are never overwritten,
    # and identical reruns reuse existing files rather than multiplying them.
    for file, content in writes:
        if file.exists() and (not file.is_file() or
                              (file.read_bytes() != content and not (file==manifest and file.read_bytes()==previous))):
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
                old = file.read_bytes()
                if file == manifest and old == previous and old != content:
                    # Publish the migrated index last and atomically. No user
                    # source or existing generated .egg is replaced.
                    with tempfile.NamedTemporaryFile(dir=manifest.parent, prefix=manifest.name+'.', delete=False) as stream:
                        replacement = Path(stream.name)
                        stream.write(content)
                    try:
                        if manifest.read_bytes() != previous:
                            raise ValueError(f'output changed while splitting: {manifest}')
                        replacement.replace(manifest)
                    finally:
                        replacement.unlink(missing_ok=True)
                elif old != content:
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
        original = native_ast(source, folder)
        report = plan(source, original)
        if before != report['source_sha256']:
            raise ValueError('source changed during parsing')
        for file in report['files']:
            check = folder/'output.egg'
            check.write_text(file['content'])
            parsed = native_ast(check, folder)
            rulesets = {c['ruleset'] for c in parsed['commands'] if c['kind']=='rule'}
            if rulesets - set(file['rulesets']):
                raise AssertionError('split output contains an unscheduled ruleset')
            if ([c['text'] for c in parsed['commands']] !=
                    [original['commands'][i]['text'] for i in file['source_ordinals']]):
                raise AssertionError('split output changed the retained command sequence')
        public = publish(report) if not args.dry_run else {**report, 'files':[
            {k:v for k,v in f.items() if k!='content'} for f in report['files']]}
    print(json.dumps(public, ensure_ascii=False))


if __name__ == '__main__':
    main()

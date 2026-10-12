"""Manipulate native-parser ASTs. No handwritten egglog parser."""
import json


def text(e, fork=False, signatures=None, params=None):
    params = params or {}
    raw = text(e) if fork or params else None
    if raw in params:
        return params[raw]
    k = e['kind']
    if k == 'var':
        return e['name']
    if k == 'literal':
        return e['text']
    if k != 'call':
        raise ValueError(f'unsupported expression {k}')
    op = e['head']
    if fork:
        if op not in signatures or signatures[op]['kind'] != 'constructor':
            raise ValueError(f'non-constructor expression {op}')
        op = 'Spine-' + op
    return '(' + ' '.join([op] + [text(a, fork, signatures, params) for a in e['args']]) + ')'


def walk(e):
    yield e
    for a in e.get('args', []):
        yield from walk(a)


def actions(expressions, fork=False, signatures=None, params=None):
    result = []
    for a in expressions:
        if a['kind'] == 'expr':
            result.append(text(a['expr'], fork, signatures, params))
        elif a['kind'] == 'union':
            result.append('(union ' + ' '.join(text(e, fork, signatures, params) for e in a['args']) + ')')
        elif a['kind'] == 'let':
            result.append(f'(let {a["name"]} {text(a["expr"], fork, signatures, params)})')
        elif a['kind'] == 'change' and not fork:
            change=a['change'].lower()
            if change not in ('subsume','delete'): raise ValueError(f'unsupported row change {change}')
            result.append(f'({change} ({a["head"]} {" ".join(text(e,params=params) for e in a["args"])}))')
        else:
            raise ValueError(f'unsupported action: {a["kind"]}')
    return result


def expressions(acts):
    for a in acts:
        if a['kind'] in ('expr', 'let'):
            yield a['expr']
        elif a['kind'] == 'union':
            yield from a['args']
        else:
            raise ValueError(f'unsupported action {a["kind"]}')


def key(obj):
    return json.dumps(obj, sort_keys=True, separators=(',', ':'))

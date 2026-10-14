"""Read-only Cell egraph views. No analysis, saturation or inferred certificates."""
import json
import os
import hashlib
import functools
import re
from pathlib import Path


def folder_in_out(root, path):
    folder = (root / path).resolve()
    if not folder.is_relative_to((root / 'out').resolve()):
        raise ValueError('请选择仓库 out/ 内的分析目录')
    return folder


def read_run(root, path, snapshot=None):
    folder = folder_in_out(root, path)
    final = folder / 'analysis.json'
    if snapshot:
        if not re.fullmatch(r'round-[0-9]+', snapshot):
            raise ValueError('无效的快照文件名')
        data = json.loads((folder / 'rounds' / (snapshot + '.json')).read_text())
    elif final.is_file():
        data = json.loads(final.read_text())
    else:
        manifest = json.loads((folder / 'rounds/manifest.json').read_text())
        if not manifest:
            raise ValueError('该次分析尚未生成状态快照')
        stem = manifest[-1]['stem']
        if not re.fullmatch(r'round-[0-9]+', stem):
            raise ValueError('无效的快照文件名')
        data = json.loads((folder / 'rounds' / (stem + '.json')).read_text())
    queue = data.get('saturated_rule_composition') or {}
    return folder, queue


def source_digest(text):
    return hashlib.sha256(text.replace('\r\n', '\n').strip().encode()).hexdigest()


@functools.lru_cache(maxsize=4096)
def file_digest(path, modified, size):
    return source_digest(Path(path).read_text())


def directories(root, source):
    wanted = source_digest(source)
    entries = []
    for current, children, files in os.walk(root / 'out', followlinks=False):
        children[:] = [c for c in children if c not in {
            'saturated-rule-composition', 'rounds', 'cells', 'states', 'node_modules',
            'target', '.git', '__pycache__'}]
        folder = Path(current)
        artifact = folder / 'analysis.json'
        if not artifact.is_file():
            artifact = folder / 'rounds/manifest.json'
        if not artifact.is_file():
            continue
        if not folder.resolve().is_relative_to((root / 'out').resolve()):
            continue
        matched = False
        for name in ['input.egg', 'source.egg']:
            path = folder / name
            try:
                if path.is_file() and path.resolve().is_relative_to((root / 'out').resolve()):
                    st = path.stat()
                    matched |= file_digest(str(path), st.st_mtime_ns, st.st_size) == wanted
            except (OSError, UnicodeError):
                pass
        entries.append({'path': str(folder.relative_to(root)),
                        'modified': artifact.stat().st_mtime, 'matches_source': matched})
    entries.sort(key=lambda e: (-e['modified'], e['path']))
    return {'directories': entries, 'source_sha256': wanted,
            'matching': 'source content, normalized line endings and outer whitespace'}


def overview(root, path, snapshot=None):
    folder, queue = read_run(root, path, snapshot)
    registry = queue.get('cell_egraph') or {}
    mapping = {v['unit']: v['eclass'] for v in registry.get('units', [])}
    jobs = {j.get('cell_unit', j.get('candidate_id')): j for j in queue.get('jobs', [])}
    nodes = []
    windows = registry.get('windows', [])
    windows_by_unit = {}
    for w in windows:
        windows_by_unit.setdefault(w['unit'], []).append(w)
    for n in registry.get('nodes', []):
        j = jobs.get(n['unit'], {})
        export = j.get('export') or {}
        nodes.append(dict(n, eclass=mapping.get(n['unit']), state=j.get('state', 'Unknown'),
            observed=bool(export.get('observed_state')), reason=j.get('reason'),
            history_windows=windows_by_unit.get(n['unit'], []),
            symbolic=j.get('origin', {}).get('symbolic_lowering')))
    manifest = folder / 'rounds/manifest.json'
    snapshots = json.loads(manifest.read_text()) if manifest.is_file() else []
    return {'path': str(folder.relative_to(root)), 'snapshot': snapshot, 'snapshots': snapshots, 'nodes': nodes,
            'units': registry.get('units', []), 'certified_unions': registry.get('certified_unions', 0),
            'scope': registry.get('scope'), 'boundary': queue.get('boundary'),
            'reuse_value': registry.get('reuse_value'), 'windows': windows,
            'graft_interfaces': registry.get('graft_interfaces', []),
            'dsl': '(CellParents (NoParents) (Parent Cell CellParents))\n(Cell (CoarseApply CellParents i64))',
            'note': '虚线是观测历史，不代表必要依赖。绿色接口经类型、绑定、回放与两端饱和验证；缺少接口不表示不能嫁接。构造数不是全路径数。'}


def parse_term(text):
    tokens = re.findall(r'"(?:\\.|[^"\\])*"|[()]|[^\s()]+', text)
    pos = 0
    def read():
        nonlocal pos
        if pos >= len(tokens): raise ValueError('incomplete term')
        t = tokens[pos]; pos += 1
        if t != '(': return t
        values = []
        while pos < len(tokens) and tokens[pos] != ')': values.append(read())
        if pos >= len(tokens): raise ValueError('unclosed term')
        pos += 1
        return values
    result = read()
    if pos != len(tokens): raise ValueError('multiple terms')
    return result


def match_terms(state, terms):
    """Ground terms only: preserve arguments and literals; never match by op alone."""
    rows = state.get('rows', [])
    hidden = set(state.get('subsumed_rows', []))
    literals = {}
    for i, value in enumerate(state.get('values', [])):
        if value.get('literal') is not None:
            literals.setdefault(value['literal'], set()).add(i)
    by_op = {}
    for i, row in enumerate(rows):
        if i not in hidden: by_op.setdefault(row['op'], []).append((i, row))
    memo = {}
    def match(term):
        key = json.dumps(term)
        if key in memo: return memo[key]
        if isinstance(term, str):
            return {v: set() for v in literals.get(term, [])}
        if not term or not isinstance(term[0], str): return {}
        args = [match(a) for a in term[1:]]
        found = {}
        for i, row in by_op.get(term[0], []):
            if len(row['args']) != len(args) or not all(v in a for v, a in zip(row['args'], args)): continue
            evidence = {i}
            found.setdefault(row['result'], set()).update(evidence)
        memo[key] = found
        return found
    selected, unresolved = set(), []
    for text in terms:
        try: found = match(parse_term(text))
        except (ValueError, RecursionError): found = {}
        if not found: unresolved.append(text)
        for ids in found.values(): selected.update(ids)
    return sorted(selected), unresolved


def state_detail(root, path, unit, snapshot=None):
    folder, queue = read_run(root, path, snapshot)
    registry = queue.get('cell_egraph') or {}
    node = next((n for n in registry.get('nodes', []) if n['unit'] == unit), None)
    if node is None: raise ValueError('未登记的 Cell unit')
    job = next((j for j in queue.get('jobs', []) if j.get('cell_unit', j.get('candidate_id')) == unit), {})
    stored = Path(job.get('folder', ''))
    cell_folder = stored.resolve() if stored.is_absolute() else (root / stored).resolve()
    if not cell_folder.is_relative_to(folder):
        cell_folder = folder / 'saturated-rule-composition' / 'cells' / f'CSUnit-{unit:06}'
    def load(name):
        file = (cell_folder / name).resolve()
        if not file.is_relative_to(folder): raise ValueError('Cell 文件越出分析目录')
        return json.loads(file.read_text()) if file.is_file() else None
    state = None
    if job.get('state') == 'Saturated':
        for name in ['saturated-rule-composition.json', 'observed-saturated-state.json']:
            state = load(name)
            if state is not None: break
    initial = load('trigger-state.json') or load('initial-state.json')
    origin = job.get('origin') or {}
    members = origin.get('comb_members') or []
    coarse = {m['record'] for m in members if m.get('use_kind') == 'CoarseRuleComposition'}
    evidence = [e for e in origin.get('lhs_evidence', []) if e['record'] in coarse]
    grafts = [e for e in registry.get('graft_interfaces', []) if e['target'] == unit]
    jobs = {j.get('cell_unit', j.get('candidate_id')): j for j in queue.get('jobs', [])}
    def colors(parents):
        inherited = {r for p in parents for r in jobs.get(p, {}).get('origin', {}).get('members', [])}
        local = [e for e in evidence if e['record'] not in inherited]
        terms = [t['term'] for e in local for t in e.get('calls', [])]
        highlight, unresolved = match_terms(state or initial, terms) if state or initial else ([], terms)
        initial_highlight, initial_unresolved = match_terms(initial, terms) if initial else ([], terms)
        return dict(lhs_evidence=local, highlight_rows=highlight, unresolved_lhs=unresolved,
                    initial_highlight_rows=initial_highlight, initial_unresolved_lhs=initial_unresolved,
                    has_lhs_evidence=bool(local))
    historical_colors = colors(node.get('parents', []))
    grafts = [dict(e, **colors(e['parents'])) for e in grafts]
    observed = bool(state and state.get('scope', {}).get('observation_only'))
    symbolic = origin.get('symbolic_lowering') or {}
    return {'unit': unit, 'node': node, 'state': state, 'initial': initial,
        'graft_interfaces': grafts,
        'history_windows': [w for w in registry.get('windows', []) if w['unit']==unit],
        'reuse_value': next((r for r in (registry.get('reuse_value') or {}).get('cells', [])
                            if unit in r['units']), None),
        'status': job.get('state', 'Unknown'), 'reason': job.get('reason'),
        'root_record': origin.get('root_member'), 'members': members, 'preview_source': (folder / 'source.egg').read_text() if (folder / 'source.egg').is_file() else None, **historical_colors,
        'guarantee': {'replay': bool(origin.get('validation')), 'validation': origin.get('validation'),
            'saturated': state is not None, 'certified_body': state is not None and not observed,
            'observation_only': observed, 'symbolic': symbolic,
            'conditions': origin.get('analysis_program', {}).get('template', {}).get('conditions'),
            'runtime_adapter': '未提供通用运行时转移认证'},
        'color_note': '橙色为当前构造新增 coarse apply 的 LHS（按完整绑定匹配）；绿色为其余闭包，不推断逐节点 smooth 来源。'}

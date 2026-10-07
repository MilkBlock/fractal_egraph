"""Read-only Cell egraph views. No analysis, saturation or inferred certificates."""
import json
import re
from pathlib import Path


def folder_in_out(root, path):
    folder = (root / path).resolve()
    if not folder.is_relative_to((root / 'out').resolve()):
        raise ValueError('请选择仓库 out/ 内的分析目录')
    return folder


def read_run(root, path):
    folder = folder_in_out(root, path)
    data = json.loads((folder / 'analysis.json').read_text())
    queue = data.get('saturated_rule_composition') or {}
    return folder, queue


def overview(root, path):
    folder, queue = read_run(root, path)
    registry = queue.get('cell_egraph') or {}
    mapping = {v['unit']: v['eclass'] for v in registry.get('units', [])}
    jobs = {j.get('cell_unit', j.get('candidate_id')): j for j in queue.get('jobs', [])}
    nodes = []
    for n in registry.get('nodes', []):
        j = jobs.get(n['unit'], {})
        export = j.get('export') or {}
        nodes.append(dict(n, eclass=mapping.get(n['unit']), state=j.get('state', 'Unknown'),
            observed=bool(export.get('observed_state')), reason=j.get('reason'),
            symbolic=j.get('origin', {}).get('symbolic_lowering')))
    return {'path': str(folder.relative_to(root)), 'nodes': nodes,
            'units': registry.get('units', []), 'certified_unions': registry.get('certified_unions', 0),
            'scope': registry.get('scope'), 'boundary': queue.get('boundary'),
            'dsl': '(CellParents (NoParents) (Parent Cell CellParents))\n(Cell (CoarseApply CellParents i64))',
            'note': '仅显示已登记构造；未登记路径不推测。前驱入度只影响显示顺序，不代表保证强度。'}


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


def state_detail(root, path, unit):
    folder, queue = read_run(root, path)
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
    # Prefer applications added in this constructor, not inherited parent members.
    inherited = set()
    for parent in node.get('parents', []):
        pj = next((j for j in queue.get('jobs', []) if j.get('cell_unit', j.get('candidate_id')) == parent), {})
        inherited.update(pj.get('origin', {}).get('members', []))
    local_evidence = [e for e in evidence if e['record'] not in inherited]
    terms = [t['term'] for e in local_evidence for t in e.get('calls', [])]
    shown = state or initial
    highlight, unresolved = match_terms(shown, terms) if shown is not None else ([], terms)
    initial_highlight, initial_unresolved = match_terms(initial, terms) if initial is not None else ([], terms)
    observed = bool(state and state.get('scope', {}).get('observation_only'))
    symbolic = origin.get('symbolic_lowering') or {}
    return {'unit': unit, 'node': node, 'state': state, 'initial': initial,
        'status': job.get('state', 'Unknown'), 'reason': job.get('reason'),
        'members': members, 'preview_source': (folder / 'source.egg').read_text() if (folder / 'source.egg').is_file() else None, 'lhs_evidence': local_evidence, 'highlight_rows': highlight,
        'initial_highlight_rows': initial_highlight, 'initial_unresolved_lhs': initial_unresolved, 'unresolved_lhs': unresolved, 'has_lhs_evidence': bool(local_evidence),
        'guarantee': {'replay': bool(origin.get('validation')), 'validation': origin.get('validation'),
            'saturated': state is not None, 'certified_body': state is not None and not observed,
            'observation_only': observed, 'symbolic': symbolic,
            'conditions': origin.get('analysis_program', {}).get('template', {}).get('conditions'),
            'runtime_adapter': '未提供通用运行时转移认证'},
        'color_note': '橙色为当前构造新增 coarse apply 的 LHS（按完整绑定匹配）；绿色为其余闭包，不推断逐节点 smooth 来源。'}

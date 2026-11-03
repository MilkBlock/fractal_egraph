"""Explicit predicates for a displayed local slice; no scoring or trial budget."""
import collections
import re


def normalize(raw):
    raw = raw or {}
    result = {}
    for key in ('include', 'exclude', 'root_tables'):
        value = raw.get(key, [])
        if isinstance(value, str):
            value = re.split(r'[,，\s]+', value.strip())
        result[key] = sorted(set(str(v) for v in value if v))
    result['include_mode'] = raw.get('include_mode', 'all')
    if result['include_mode'] not in ('all', 'any'):
        raise ValueError('请选择全部包含或任意包含')
    result['change'] = raw.get('change', 'any')
    if result['change'] not in ('any', 'changed', 'added', 'updated', 'removed', 'union', 'unchanged'):
        raise ValueError('无效的阶段变化筛选')
    for key in ('min_alternatives', 'min_parents', 'min_facts', 'max_facts',
                'min_classes', 'max_classes', 'min_ports', 'max_ports'):
        value = raw.get(key)
        result[key] = None if value in (None, '') else int(value)
        if result[key] is not None and result[key] < 0:
            raise ValueError('筛选数量不能为负数')
    for name in ('facts', 'classes', 'ports'):
        lo, hi = result['min_'+name], result['max_'+name]
        if lo is not None and hi is not None and lo > hi:
            raise ValueError('数量范围的下限不能超过上限')
    for key in ('repeated_arg', 'shared_ports'):
        result[key] = bool(raw.get(key, False))
    return result


def conditions(f):
    checks = []
    if f['root_tables']:
        names = set(f['root_tables'])
        checks.append(('root_tables', '根含任一类型：'+', '.join(f['root_tables']), lambda m,n=names: bool(n & set(m['root_tables']))))
    if f['change'] != 'any':
        mode = f['change']
        label = {'changed':'有变化', 'added':'有新增', 'updated':'有函数／可见性更新', 'removed':'有移除', 'union':'有等价类合并', 'unchanged':'完全未变化'}[mode]
        def changed(m):
            any_change = any(m['changes'].get(k, 0) for k in ('added', 'updated', 'removed')) or bool(m['unions'])
            return not any_change if mode == 'unchanged' else bool(any_change) if mode == 'changed' else bool(m['unions']) if mode == 'union' else bool(m['changes'].get(mode))
        checks.append(('change', '局部'+label, changed))
    if f['include']:
        names = set(f['include'])
        checks.append(('include', ('全部包含：' if f['include_mode']=='all' else '任意包含：')+', '.join(f['include']),
                       lambda m,n=names: n <= set(m['tables']) if f['include_mode']=='all' else bool(n & set(m['tables']))))
    if f['exclude']:
        excluded = set(f['exclude'])
        checks.append(('exclude', '排除：'+', '.join(f['exclude']), lambda m: not excluded & set(m['tables'])))
    if f['min_alternatives']:
        checks.append(('min_alternatives', f"某类原图 enode ≥ {f['min_alternatives']}", lambda m: any(x['enodes'] >= f['min_alternatives'] for x in m['alternatives'])))
    if f['min_parents']:
        checks.append(('min_parents', f"某类被不同事实引用 ≥ {f['min_parents']}", lambda m: any(x['parents'] >= f['min_parents'] for x in m['sharing'])))
    if f['repeated_arg']:
        checks.append(('repeated_arg', '同一事实重复引用一个参数', lambda m: bool(m['repeated_args'])))
    if f['shared_ports']:
        checks.append(('shared_ports', '存在被多处引用的 RipenInput', lambda m: any(x['port'] and x['references'] > 1 for x in m['sharing'])))
    for key, label in (('facts', '展示事实'), ('classes', '展开 eclass'), ('ports', 'RipenInput')):
        lo, hi = f['min_'+key], f['max_'+key]
        if lo is not None or hi is not None:
            checks.append((key, f"{label} {lo if lo is not None else 0}–{hi if hi is not None else '不限'}",
                           lambda m,k=key,l=lo,h=hi: (l is None or m[k]>=l) and (h is None or m[k]<=h)))
    return checks


def describe(root, selected, expanded, frontier, tables, values, enode_counts, root_types, changes, union_classes):
    refs = collections.defaultdict(list)
    repeats = []
    shown = collections.Counter()
    for rid, (t, vs, _) in selected.items():
        if tables[t]['kind']=='constructor':
            shown[vs[-1]]+=1
        positions = collections.defaultdict(list)
        for i, v in enumerate(vs[:-1]):
            if 'eq' in values[v]:
                refs[v].append((rid, i))
                positions[v].append(i)
        repeats.extend({'row':rid, 'class':v, 'positions':ps} for v, ps in positions.items() if len(ps)>1)
    return {
        'root_tables': root_types.get(root, []),
        'tables': sorted({tables[r[0]]['name'] for r in selected.values()}),
        'facts': len(selected), 'classes': len(expanded), 'ports': len(frontier),
        'changes': dict(collections.Counter(changes[r] for r in selected.values())),
        'unions': sorted(union_classes & (expanded | frontier)),
        'alternatives': [{'class':v, 'enodes':enode_counts.get(v,0), 'shown':shown[v]} for v in sorted(expanded)],
        'sharing': [{'class':v, 'parents':len({rid for rid,_ in rs}), 'references':len(rs), 'port':v in frontier,
                     'rows':sorted({rid for rid,_ in rs})} for v,rs in sorted(refs.items()) if len(rs)>1],
        'repeated_args': repeats,
    }

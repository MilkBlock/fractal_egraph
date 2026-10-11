"""Exact typed ground-constructor graphs, modulo eclass IDs (Math by default)."""
import json
from collections import Counter
from pathlib import Path


def verify(left_path, right_path, eq_sorts=None):
    typed = eq_sorts is not None
    eq_sorts = set(eq_sorts or ['Math'])
    left = json.loads(Path(left_path).read_text())
    right = json.loads(Path(right_path).read_text())
    result = lambda r: (r.get('sort', 'Math'), r['result'])
    index = {(r['op'], tuple(map(tuple, r['args']))): result(r) for r in right}
    assert len(index) == len(right) == len(left), 'different enode counts or duplicate keys'
    mapping, inverse, pending = {}, {}, left[:]
    while pending:
        later = []
        for r in pending:
            if any(t in eq_sorts and (t, v) not in mapping for t, v in r['args']):
                later.append(r)
                continue
            key = (r['op'], tuple((t, mapping[t, v][1] if t in eq_sorts else v) for t, v in r['args']))
            assert key in index, 'missing constructor fact'
            target, source_id = index[key], result(r)
            assert target[0] == source_id[0], 'constructor result sort changed'
            assert source_id not in mapping or mapping[source_id] == target, 'different equivalence'
            assert target not in inverse or inverse[target] == source_id, 'non-bijective class map'
            mapping[source_id], inverse[target] = target, source_id
        assert len(later) < len(pending), 'unmapped cycle or ungrounded class'
        pending = later
    assert len(inverse) == len({result(r) for r in right})
    report = dict(exact_equal=True, enodes=len(left), eclasses=len(mapping))
    if typed:
        nodes = Counter(r.get('sort', 'Math') for r in left)
        classes = Counter(t for t, _ in mapping)
        report['sorts'] = {t: dict(enodes=nodes[t], eclasses=classes[t]) for t in sorted(nodes)}
    return report

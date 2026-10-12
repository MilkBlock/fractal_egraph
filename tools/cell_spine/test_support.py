"""Tests query the selected marker ABI, whose arity is solver-controlled."""
import json
from .terms import text


def marker_pattern(output, family, bindings=None):
    layout=json.loads((output/'report.json').read_text())['retirement']['marker_layouts'][str(family)]
    fields={name:f'marker_field_{i}' for i,name in enumerate(layout['fields'])}
    fields.update(bindings or {})
    args=[text(term,params=fields) for term in layout['terms']]
    if layout.get('read_storage','canonical')=='canonical':
        args=[arg.replace('(Spine-', '(Spine-Cano-') for arg in args]
    return f'(Spine-Summary-{family} '+ ' '.join(args)+')'

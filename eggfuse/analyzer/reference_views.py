"""Exact storage projections, independent of closure or query completeness.

A carrier is a row already read by a successful match. Its slot program can
represent that same row without storing another tuple. Absorption still checks
all live rows and referenced old positions at the publication boundary.
"""
from eggfuse.ir.recipe import digest

SCHEMA='eggfuse-reference-views/v1'

def build(recipe, marker_tables=()):
    tables={t['name']:t for t in recipe['tables']}
    markers=sorted(set(marker_tables))
    def mentions(x,name):
        if isinstance(x,dict):return x.get('table')==name or any(mentions(v,name) for v in x.values())
        if isinstance(x,list):return any(mentions(v,name) for v in x)
        return False
    for name in markers:
        t=tables[name]
        if t['kind']=='constructor' or t['schema'][-1]['kind']!='Unit' or t.get('merge_expr')!={'var':'old'}:raise ValueError('reference marker requires inert Unit merge-old')
        if any(mentions(t.get('merge_expr'),name) for t in tables.values()):raise ValueError('marker observable from merge')
        if any(e.get('call',{}).get('table')==name and e['op']!='set' for r in recipe['rules'] for e in r['effects']):raise ValueError('marker has observable RHS read or removal')
    rules=[]
    for rid,r in enumerate(recipe['rules']):
        carriers=[];projection={}
        for ai,a in enumerate(r['query']):
            name=a['call'].get('table')
            if name is None:continue
            t=tables[name]
            if not(t['kind']=='constructor' or (t['schema'][-1]['kind']=='Unit' and t.get('merge_expr') is None) or name in markers):continue
            carriers.append(dict(atom=ai,table=name,columns=a['args']))
            for col,v in enumerate(a['args']):
                if 'slot' in v:projection.setdefault(v['slot'],dict(slot=v['slot'],atom=ai,column=col))
        exports={}
        effect_rows=[]
        for oi,e in enumerate(r['effects']):
            name=e.get('call',{}).get('table')
            if not name:continue
            t=tables[name]
            if not(t['kind']=='constructor' or (t['schema'][-1]['kind']=='Unit' and t.get('merge_expr') is None) or name in markers):continue
            if e['op']=='let':cols=e['args']+[dict(slot=e['dst'])]
            elif e['op']=='set':cols=e['args']+[e['value']]
            else:continue
            effect_rows.append((oi,cols))
        effect_rows.extend((len(r['effects'])+i,c['columns']) for i,c in enumerate(carriers))
        for oi,cols in effect_rows:
            for col,v in enumerate(cols):
                if 'slot' in v:exports.setdefault(v['slot'],[]).append(dict(op=oi,column=col))
        marker_only=any(e['op']=='set' and e.get('call',{}).get('table') in markers for e in r['effects']) and all(e['op']=='copy' or (e['op']=='let' and 'primitive' in e.get('call',{})) or (e['op']=='set' and e.get('call',{}).get('table') in markers) for e in r['effects'])
        rules.append(dict(rule=rid,storage='reference-view' if marker_only else 'owned-recipe',carriers=carriers,slots=list(projection.values()),
            continuation_slots=[dict(slot=s,sources=cols) for s,cols in sorted(exports.items())]))
    return dict(schema=SCHEMA,recipe_sha256=recipe['sha256'],marker_tables=markers,rules=rules,
        contract=dict(projection='immutable matched row coordinates; mutable ports keep copied values',
            containment='all owned live facts and retained parameter references must match target carriers',
            publication='relocate only after Run RHS and repair; preserve fact epoch/born/hidden',
            retirement='redirect references before removing empty source frame; no logical subsume',
            fallback='retain source storage when any obligation is not covered'))

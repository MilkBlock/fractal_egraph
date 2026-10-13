"""Shared seed decoders, guarded by the original live canonical marker.

These are positive materialized views, not resurrected child Summaries. They
may retain rows after retirement; every consumer still reads the owner marker.
Union/rebuild can add bindings through the normal egglog incremental schedule.
"""
from . import names, takeover as t
from .terms import text, walk


def name(family):return f'Interface-{family}!'


def is_interface(head):return head.startswith('Interface-') and head.endswith('!') and head[10:-1].isdigit()


def coordinates(layout):
    result=[t.var(f) for f in layout['fields']]
    # Include intermediate values so consumers can reuse actual subtrees on
    # their RHS, without decoding the forest a second time.
    seen={text(e) for e in result+layout['roots']}
    for q in layout['decode']:
        for root in q['args']:
            for e in walk(root):
                if e['kind']=='call' and text(e) not in seen:
                    seen.add(text(e));result.append(e)
    return result


def storage_columns(layout):return layout['columns']+layout.get('interface_columns',0)


def extend(bundle,mode='auto'):
    if mode not in ('inline','auto','shared'):raise ValueError('unknown interface mode')
    if mode=='inline':return dict(bundle,interface_mode=mode)
    chosen={};consumers=list(bundle['consumers'])
    for family,c in bundle['chosen'].items():
        layouts=[];has_shared=False
        fields={f'r{i}':c['signatures'][r['head']]['output'] for i,r in enumerate(c['roots'])}
        fields.update({v:c['param_sorts'][k] for k,v in c['params'].items()})
        for l in c['marker_layouts']:
            if mode=='auto' or not l['decode']:layouts.append(l)
            if not l['decode']:continue
            coords=coordinates(l)
            types=[fields[e['name']] if e['kind']=='var' else c['signatures'][e['head'][:-1]]['output'] for e in coords]
            layouts.append(dict(l,id=l['id']+'_shared',interface='shared',interface_types=types,
                                interface_columns=len(coords)))
            has_shared=True
        chosen[family]=dict(c,marker_layouts=layouts)
        if has_shared:
            # Placeholder is costed/lowered for the SAME global layout choice
            # as all consumers; inline layouts contribute no rule or view.
            consumers.append(dict(name=f'interface_{family}',ruleset='Run!',interface_family=family,
                body=[dict(kind='fact',expr=t.call(names.summary(family),map(t.var,c['marker_layouts'][0]['fields'])))],head=[]))
    return dict(bundle,chosen=chosen,consumers=consumers,interface_mode=mode)


def producer(rule,selected,signatures):
    from . import markers
    family=rule['interface_family'];layout=selected[family]
    if layout.get('interface')!='shared':return dict(rule,body=[],head=[],disabled=True,reused_subtrees=[])
    source=dict(rule);source.pop('interface_family')
    source['head']=[dict(kind='expr',expr=t.call(name(family),coordinates(layout)))]
    return markers.lower_ast(source,{family:dict(layout,interface='inline')},signatures)


def declarations(selected,signatures):
    sorts={s['output'] for s in signatures.values()}
    return [f'(relation {name(f)} ({" ".join(names.raw(s) if s in sorts else s for s in l["interface_types"])}))'
            for f,l in selected.items() if l.get('interface')=='shared']

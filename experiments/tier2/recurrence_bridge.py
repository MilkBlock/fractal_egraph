"""Native tier-1 interface adapter for the finite additive-recurrence fixture.
Coordinates are (remaining argument, accumulated edge weight), not raw runtime
bindings. Each edge has separately passed a native tier-0 endpoint check.
"""
import json
from pathlib import Path
from affine import path_samples
OUT=Path(__file__).parent
q=json.dumps

def val(x):return f'(V "i64" "{x}")'
def args(xs):
    out='(ANil)'
    for x in reversed(xs):out=f'(ACons {val(x)} {out})'
    return out
text=['(include "experiments/tier1_effects/tier1_rule_comb_ir.egg")',
      '(relation Coordinates (Instance i64 i64))','(relation SeriesStep (Instance String i64))',
      '(relation ObservedStep (String i64 i64 i64 i64 i64))',
      '(rule ((LinkedParent child 0 parent) (Coordinates parent m a) (Coordinates child m2 a2) (SeriesStep child s k)) ((ObservedStep s k m a m2 a2)) :ruleset tier1)',
      '(let $base (CoarseComb (MoreParents (Empty) (NoParents)) (Rule "boundary") (PCons (External 0 "i64") (PCons (External 1 "i64") (PNil)))))']
checks=[];event=0
for name in ['unit','triple','stride','changing','warmup']:
    data=json.loads((OUT/'fixtures'/f'{name}.json').read_text());samples=path_samples(data['edges'],24)
    parent=None
    for depth,state in enumerate([samples[0][0]]+[b for a,b in samples]):
        ident=f'$o{event}';comb=f'$c{event}'
        if parent is None:
            text.append(f'(let {comb} $base)')
        else:
            previous=samples[depth-1][0];delta=[b-a for a,b in zip(previous,state)]
            ports=[]
            for slot,d in enumerate(delta):
                ports.append(f'(Make "+" (RCons (ParentPort 0 {slot} "i64") (RCons (Make "literal:{d}" (RNil) "i64") (RNil))) "i64")')
            text.append(f'(let {comb} (SmoothComb (MoreParents {parent[1]} (NoParents)) (Rule "additive-unfold") (RCons {ports[0]} (RCons {ports[1]} (RNil)))))')
        text.append(f'(let {ident} (Occurrence {event} {comb}))')
        text.append(f'(Coordinates {ident} {state[0]} {state[1]})')
        text.append(f'(SeriesStep {ident} "{name}" {depth})')
        for slot,v in enumerate(state):text.append(f'(OutputAt {ident} {slot} {val(v)})')
        if parent is None:
            for slot,v in enumerate(state):text.append(f'(ExternalAt {ident} {slot} {val(v)})')
        else:
            text.append(f'(ParentAt {ident} 0 {parent[0]})')
            for slot,d in enumerate(delta):
                text.append(f'(Materialized {ident} "literal:{d}" (ANil) {val(d)})')
                text.append(f'(Materialized {ident} "+" {args([previous[slot],d])} {val(state[slot])})')
        checks.append(f'(check (Binding {ident} {args(state)}))')
        parent=(ident,comb);event+=1
text+=['(run-schedule (saturate (run tier1)))']+checks
(OUT/'recurrence_tier1.egg').write_text('\n'.join(text)+'\n')

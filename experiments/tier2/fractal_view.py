"""Occurrence-faithful, maximal-chain view of the witnessed HigherRule region."""
import json
from pathlib import Path
from collections import defaultdict
OUT=Path(__file__).parent

def pretty(e):
    if 'var' in e:return e['var']
    if 'literal' in e:return e['literal']
    a=[pretty(x) for x in e['args']];op=e['op']
    if op in {'Add','Sub','Mul','Div','Pow'} and len(a)==2:
        return '('+a[0]+' '+{'Add':'+','Sub':'−','Mul':'·','Div':'/','Pow':'^'}[op]+' '+a[1]+')'
    if op=='Integral':return '∫('+a[0]+') d'+a[1]
    if op=='Diff':return 'D('+a[1]+', '+a[0]+')'
    return op+'('+', '.join(a)+')'
def readable_update(extension,rules):
    out=[]
    for slot,route in zip(extension['schema']['inputs'],extension['routes']):
        if 'variable' not in slot:continue
        if route[0]=='ParentRole':
            rule,role=route[2].split(':',1)
            if role.startswith('variable:'):meaning=role.split(':',1)[1]
            else:
                path,op,col=role.rsplit(':',2);expr=rules[rule]
                for key in path.strip('/').split('/'):expr=expr[int(key)] if isinstance(expr,list) else expr[key]
                n=int(col);meaning=pretty(expr['args'][n] if n<len(expr['args']) else expr)
            out.append(slot['variable']+' ← '+meaning)
        else:out.append(slot['variable']+' ← 外部接口')
    return out

def build():
    math=json.loads((OUT/'math.json').read_text());occ={x['event']:x for x in math['occurrences']};ext={x['name']:x for x in math['extensions']}
    steps={x['event']:x for x in json.loads((OUT/'higher_steps.json').read_text())}
    higher=json.loads((OUT/'higher_native.json').read_text())['higher_rules']
    t1=OUT.parent/'tier1_extract';defs=json.loads((t1/'extraction.json').read_text())['definitions'];byclass={x['eclass']:x for x in defs}
    rules=json.loads((t1/'native_interfaces.json').read_text())['rules']
    results={x['name']:x for x in json.loads((t1/'combined.json').read_text())['combinations']}
    children=defaultdict(list)
    for event,s in steps.items():
        parents=occ[event]['parents'];assert len(parents)==1
        p=parents[0]
        if p in steps and steps[p]['extension']==s['extension']:children[p].append(event)
    paths=[]
    def walk(event,path):
        assert event not in path[:-1],'cycle'
        if not children[event] and len(path)>=2:paths.append(path)
        for c in sorted(children[event]):walk(c,path+[c])
    for event in sorted(steps):
        p=occ[event]['parents'][0]
        if p not in steps or steps[p]['extension']!=steps[event]['extension']:walk(event,[event])
    paths.sort(key=lambda p:(-len(p),p))
    lanes=[];nodes={}
    def node(event):
        if event in nodes:return
        o=occ[event];d=byclass[o['template']];r=results[d['name']]
        variant=next((v for v in r['variants'] if event in v['occurrences']),None)
        failure=next((v for v in r['other_occurrences'] if v['event']==event),None)
        e=ext[o['extension']]
        nodes[event]={'event':event,'comb':d['name'],'rule':d.get('tier0_rule_id'),'kind':d['kind'],'depth':o['depth'],
            'source':d.get('tier0_definition','').replace('\\n','\n'),'tier1':d['expression'],
            'equation':pretty(rules[d['tier0_rule_id']]['body'][0]['eq'][1])+' ⇒ '+pretty(rules[d['tier0_rule_id']]['head'][0]['union'][1]),
            'combined':variant['egg'] if variant else None,'reason':failure.get('reason',failure['status']) if failure else None,
            'parent_events':o['parents'],'external_routes':[r for r in e['routes'] if r[0]=='Outside'],
            'source_routes':e['routes']}
    for index,path in enumerate(paths):
        start=path[0];trigger=occ[start]['parents'][0];operator=steps[start]['extension'];ctx=byclass[occ[trigger]['template']]['name']
        targets=[steps[e]['out'] for e in path]
        witness=next((h for h in higher if h['ctx']==ctx and h['operator']==operator and h['count']==len(path) and targets[-1] in h['represents']),None)
        assert witness,'must be an existing native HigherRule'
        for event in [trigger]+path:node(event)
        lanes.append({'id':f'chain_{index+1}','operator':operator,'rule':ext[operator]['rule'],'trigger':trigger,'events':path,'higher':witness['egg'],'update':readable_update(ext[operator],rules)})
    # Every finite macro view must be represented by a contiguous segment of a lane.
    for h in higher:
        assert any(h['operator']==l['operator'] and any(
            nodes[([l['trigger']]+l['events'])[i]]['comb']==h['ctx'] and
            nodes[l['events'][i+h['count']-1]]['comb'] in h['represents']
            for i in range(len(l['events'])-h['count']+1)) for l in lanes),'unrepresented HigherRule'
    return {'lanes':lanes,'nodes':nodes,'stats':{'higher_rules':len(higher),'maximal_chains':len(lanes),
        'applications':sum(len(l['events']) for l in lanes),'visible_unique_contexts':len({x['comb'] for x in nodes.values()}),'total_comb_templates':len(defs)},
        'scope':'Focused view of witnessed fractal regions, not a lossless compression of the whole tier-0 egraph. Ellipses are unverified continuation; trigger is the observed preceding context.'}
if __name__=='__main__':
    data=build();(OUT/'fractal_view.json').write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
    template=(OUT/'fractal_view_template.html').read_text();payload=json.dumps(data,ensure_ascii=False).replace('<','\\u003c')
    (OUT/'fractal.html').write_text(template.replace('__DATA__',payload));print(data['stats'])

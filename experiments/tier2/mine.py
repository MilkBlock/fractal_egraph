"""Describe real tier-1 extensions without enumerating new combined rules.
Numeric ParentPort indices are resolved to producer source-AST output roles.
This first pass groups exact interface/effect schemas; it is not anti-unification.
"""
import sys,json,hashlib
from pathlib import Path
from collections import defaultdict,Counter
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'experiments/tier1_extract'))
from lower import Lower
OUT=Path(__file__).parent

def canonical(ast):
    names={}
    def go(x):
        if isinstance(x,list):return [go(v) for v in x]
        if not isinstance(x,dict):return x
        if 'var' in x:return {'var':names.setdefault(x['var'],len(names))}
        return {k:go(v) for k,v in x.items() if k!='span'}
    return go(ast)
def paths(ast):
    result={}
    def walk(x,path):
        if isinstance(x,dict):
            if 'span' in x:result[x['span']]=path
            for k,v in x.items():
                if k!='span':walk(v,path+'/'+k)
        elif isinstance(x,list):
            for k,v in enumerate(x):walk(v,path+'/'+str(k))
    walk(ast,'');return result

def describe(engine,i):
    tree=engine.tree(i['template']);kind,parents,rule,binding=tree
    rid=rule[1];source=engine.rules[rid];roles=paths(source)
    slots=engine.sequence(binding,'PCons' if kind=='CoarseRuleComposition' else 'RCons','PNil' if kind=='CoarseRuleComposition' else 'RNil')
    def route(x):
        if x[0]=='Local':return route(x[1])
        if x[0]=='External':return ['Outside',x[1],x[2]]
        if x[0]=='ParentPort':
            _,parent,slot,sort=x;p=engine.occ[i['parents'][parent]];pr=engine.tree(p['template'])[2][1]
            layout=p['outputs'][slot];pp=paths(engine.rules[pr])
            if layout.startswith('variable:'):
                # Preserve producer variable identity via its source AST, not value ID.
                role=layout
            elif layout.startswith('produced-row:'):role='row:'+pp[layout[len('produced-row:'):]]
            else:
                span,op,col=layout.rsplit(':',2);role=pp[span]+':'+op+':'+col
            return ['ParentRole',parent,pr+':'+role,sort]
        if x[0] in ('Make','MakePartial'):
            cons,nil=('RCons','RNil') if x[0]=='Make' else ('PCons','PNil')
            return ['Construct',x[1],[route(y) for y in engine.sequence(x[2],cons,nil)],x[3]]
        raise ValueError(x)
    schema={'kind':kind,'rule':canonical(source),'inputs':[{'variable':s['variable']} if 'variable'in s else {'read':roles[s['read_span']],'op':s['op']} for s in i['inputs']]}
    return {'rule':rid,'routes':[route(x) for x in slots],'schema':schema}
def quote(x):return json.dumps(x,ensure_ascii=False)
def route_egg(r):
    if r[0]=='Construct':return f'(Construct {quote(r[1])} {routes_egg(r[2])} {quote(r[3])})'
    return '('+r[0]+' '+' '.join(str(v) if isinstance(v,int) else quote(v) for v in r[1:])+')'
def routes_egg(rs):
    out='(End)'
    for r in reversed(rs):out=f'(Next {route_egg(r)} {out})'
    return out

def main():
    p=ROOT/'experiments/tier1_extract';native=json.loads((p/'native_templates.json').read_text());interfaces=json.loads((p/'native_interfaces.json').read_text())
    assert hashlib.sha256((p/'native_templates.json').read_bytes()).hexdigest()==interfaces['native_sha256']
    engine=Lower(native,interfaces,interfaces['rules']);occ=sorted(engine.occ.values(),key=lambda i:i['event'])
    depth={}
    for i in occ:
        assert all(p<i['event'] for p in i['parents'])
        depth[i['event']]=1+max((depth[p] for p in i['parents']),default=0)
    groups={};records=[]
    text=['(include "experiments/tier2/ir.egg")']
    for i in occ:
        d=describe(engine,i);key=json.dumps(d,sort_keys=True)
        if key not in groups:
            name=f'ext_{len(groups):04d}';groups[key]={'name':name,**d,'events':[]}
            text.append(f'(let ${name} (Extend {quote(d["rule"])} {routes_egg(d["routes"])} (Schema {quote(json.dumps(d["schema"],sort_keys=True))})))')
        g=groups[key];g['events'].append(i['event'])
        if d['schema']['kind']=='CoarseRuleComposition':text.append(f'(NeedsBoundary ${g["name"]})')
        records.append({'event':i['event'],'template':i['template'],'extension':g['name'],'depth':depth[i['event']],'parents':i['parents']})
        text.append(f'(At {i["event"]} ${g["name"]})')
        for slot,parent in enumerate(i['parents']):text.append(f'(Before {parent} {i["event"]} {slot})')
    text.append('(run-schedule (saturate (run tier2)))')
    (OUT/'math.egg').write_text('\n'.join(text)+'\n')
    for g in groups.values():
        g['train_depth_le_3']=sum(depth[e]<=3 for e in g['events']);g['heldout_depth_gt_3']=sum(depth[e]>3 for e in g['events'])
    report={'scope':'Exact extension/interface/effect schema reuse on native Math tier-1. No infinite-law proof. Depth split reuses one workload and is not an independent workload test.',
        'summary':{'occurrences':len(occ),'extensions':len(groups),'edges':sum(len(i['parents']) for i in occ),'max_depth':max(depth.values()),
        'heldout_occurrences':sum(d>3 for d in depth.values()),'heldout_seen_schema':sum(g['heldout_depth_gt_3'] for g in groups.values() if g['train_depth_le_3'])},
        'extensions':sorted(groups.values(),key=lambda g:(-len(g['events']),g['name'])),'occurrences':records}
    (OUT/'math.json').write_text(json.dumps(report,indent=2)+'\n');print(report['summary'])
if __name__=='__main__':main()

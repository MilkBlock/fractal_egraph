"""Interpret selected native Comb constructors and their typed output interfaces.
No candidate search, external composer, or equality inference from concrete values.
Keep all constructor/union effects in chronological SSA form. Unsupported dynamic
coarse joins are reported, never silently exported as fused rules.
"""
import json,hashlib
from collections import defaultdict,Counter
from pathlib import Path
from extract import select
OUT=Path(__file__).parent
class Unsupported(Exception):pass
class Lower:
    def __init__(self,native,interfaces,rules):
        self.nodes=native['native_egraph']['nodes'];self.chosen,_=select(self.nodes)
        self.occ={i['event']:i for i in interfaces['occurrences']};self.rules=rules
    def node(self,c):return self.nodes[self.chosen[c]]
    def tree(self,c):
        n=self.node(c)
        if c.split('-',1)[0] in ('String','i64'):return json.loads(n['op'])
        return [n['op']]+[self.tree(self.nodes[x]['eclass']) for x in n['children']]
    def reset(self):
        self.uf={};self.entry=set();self.bound=set();self.rows=[];self.body=[];self.head=[];self.memo={};self.steps=[];self.guard_count=0
    def fresh(self,entry=False):
        v='v'+str(len(self.uf));self.uf[v]=v
        if entry:self.entry.add(v)
        return v
    def find(self,v):
        if v not in self.uf:return v
        while self.uf[v]!=v:v=self.uf[v]
        return v
    def union(self,a,b):
        a,b=self.find(a),self.find(b)
        if a==b:return
        # Literal or entry representatives make later bindings readable; never
        # rewrite already emitted LHS facts or RHS actions using a later union.
        if a in self.uf and (b not in self.uf or b in self.entry and a not in self.entry):a,b=b,a
        if b in self.uf:self.uf[b]=a
        else:raise Unsupported('incompatible literal equality')
    def constrain(self,a,b):
        a,b=self.find(a),self.find(b)
        if a==b:return
        for x,y in [(a,b),(b,a)]:
            if x in self.entry and x not in self.bound:
                self.uf[x]=y;return
        if all(x not in self.uf or x in self.entry for x in [a,b]):
            self.body.append(f'(= {a} {b})');self.guard_count+=1;self.union(a,b);return
        raise Unsupported('coarse join needs equality with an intermediate result; staged matching required')
    def call(self,op,args):return '('+op+(' '+' '.join(args) if args else '')+')'
    def available(self,op,args):
        return next((i for i,(o,aa,v) in enumerate(self.rows) if o==op and [self.find(x) for x in aa]==[self.find(x) for x in args]),None)
    def expr(self,e,env,reads,calls,head=False):
        if 'literal' in e:return e['literal']
        if 'var' in e:
            v=e['var']
            if v not in env:env[v]=self.fresh(entry=not head)
            return self.find(env[v])
        op=e['op']
        if op not in {'Add','Mul','Sub','Div','Pow','Diff','Integral','Sin','Cos','Ln','Sqrt','Const','Var'}:
            raise Unsupported('not a Math constructor: '+op)
        args=[self.expr(x,env,reads,calls,head) for x in e['args']]
        span=e['span']
        witness=reads.get(span)
        if not head and witness is not None:
            if not isinstance(witness,tuple) or witness[0]!='row':raise Unsupported('invalid fact port')
            idx=witness[1];o,aa,out=self.rows[idx]
            if o!=op or len(aa)!=len(args):raise Unsupported('fact port operator mismatch')
            for a,b in zip(args,aa):self.constrain(a,b)
        else:
            idx=self.available(op,args)
            if idx is None:
                if not head and any(self.find(a) in self.uf and self.find(a) not in self.entry for a in args):
                    raise Unsupported('coarse read consumes a new intermediate value; staged matching required')
                args=[self.find(a) for a in args];out=self.fresh(entry=not head)
                idx=len(self.rows);self.rows.append((op,args,out))
                if head:self.head.append(f'(let {out} {self.call(op,args)})')
                else:
                    self.body.append(f'(= {out} {self.call(op,args)})');self.bound.update(args+[out])
            else:out=self.rows[idx][2]
        calls[span]=idx
        return self.find(out)
    @staticmethod
    def sequence(t,cons,nil):
        out=[]
        while t[0]==cons:out.append(t[1]);t=t[2]
        assert t==[nil],t
        return out
    def apply(self,event):
        if event in self.memo:return self.memo[event]
        i=self.occ[event];t=self.tree(i['template']);kind=t[0]
        if kind not in ('SmoothRuleComposition','CoarseRuleComposition'):raise Unsupported('non-application template')
        parent_trees=self.sequence(t[1],'MoreParents','NoParents')
        real=[self.occ[p]['template'] for p in i['parents']]
        assert parent_trees==([self.tree(c) for c in real] or [['Empty']]),'parent occurrence/template mismatch'
        parents=[self.apply(p) for p in i['parents']]
        rid=t[2][1];rule=self.rules[rid]
        ports=self.sequence(t[3],*('RCons','RNil') if kind=='SmoothRuleComposition' else ('PCons','PNil'))
        assert len(ports)==len(i['inputs']),'input schema mismatch'
        env={};reads={};external={};calls={}
        for port,slot in zip(ports,i['inputs']):
            if port[0]=='Local':port=port[1]
            if port[0]=='ParentPort':
                _,p,n,sort=port;v=parents[p][n]
                assert isinstance(v,tuple)==sort.startswith('Fact:'),'typed output mismatch'
            elif port[0]=='External':
                _,n,sort=port
                if n not in external:external[n]=None if sort.startswith('Fact:') else self.fresh(entry=True)
                v=external[n]
            else:raise Unsupported('unsupported relative-binding constructor '+port[0])
            if 'variable' in slot:
                if v is None or isinstance(v,tuple):raise Unsupported('variable is not a value port')
                env[slot['variable']]=v
            else:reads[slot['read_span']]=v
        for fact in rule['body']:
            if 'eq' not in fact:raise Unsupported('non-equality source guard')
            a,b=fact['eq']
            # Bind a fresh root to the witnessed constructor output, not to a
            # concrete trace value. Repeated variables add genuine constraints.
            if 'var' in a and a['var'] not in env:env[a['var']]=self.expr(b,env,reads,calls)
            else:self.constrain(self.expr(a,env,reads,calls),self.expr(b,env,reads,calls))
        if set(reads)-set(calls):raise Unsupported('input witness is not mapped to a source LHS call')
        for action in rule['head']:
            if 'union' not in action:raise Unsupported('unsupported source action')
            a,b=[self.expr(x,env,reads,calls,True) for x in action['union']]
            if self.find(a)!=self.find(b):self.head.append(f'(union {a} {b})');self.union(a,b)
        outputs=[]
        for layout in i['outputs']:
            if layout.startswith('variable:'):v=env[layout[len('variable:'):]]
            elif layout.startswith('produced-row:'):v=('row',calls[layout[len('produced-row:'):]])
            else:
                span,op,col=layout.rsplit(':',2);idx=calls[span];o,args,out=self.rows[idx]
                assert op==o
                v=(args+[out])[int(col)]
            outputs.append(v)
        self.memo[event]=outputs;self.steps.append({'event':event,'template':i['template'],'rule':rid})
        return outputs
    def lower(self,event,name):
        self.reset()
        try:
            self.apply(event)
            if not self.head:return {'status':'no_new_effect','event':event,'steps':self.steps}
            egg='(rule (\n  '+'\n  '.join(dict.fromkeys(self.body))+'\n) (\n  '+'\n  '.join(self.head)+'\n) :ruleset tier1-combined :name "'+name+'")'
            return {'status':'lowered','event':event,'steps':self.steps,'guard_count':self.guard_count,'egg':egg,
                'lhs_facts':len(set(self.body)),'rhs_actions':len(self.head),'body':self.body,'head':self.head}
        except (Unsupported,KeyError,IndexError) as e:return {'status':'needs_staged_matching','event':event,'reason':str(e),'steps':self.steps}

def main(selected=None):
    n=json.loads((OUT/'native_templates.json').read_text());interfaces=json.loads((OUT/'native_interfaces.json').read_text());rules=interfaces['rules']
    assert interfaces['native_sha256']==hashlib.sha256((OUT/'native_templates.json').read_bytes()).hexdigest()
    engine=Lower(n,interfaces,rules);report=json.loads((OUT/'extraction.json').read_text())
    by_class=defaultdict(list)
    for i in interfaces['occurrences']:by_class[i['template']].append(i['event'])
    results=[];text=['; Whole combinations interpreted from selected native tier-1 Comb constructors.',
        '; Every occurrence is checked. Variants preserve different DAG sharing/interfaces.',
        '; Effects include intermediate constructors and unions. Additional entry guards are reported.',
        '(ruleset tier1-combined)']
    # One datatype; no source rules, seeds, or schedule.
    datatype=next(x for x in (OUT/'tier0_rules.egg').read_text().splitlines() if x.startswith('(datatype Math '))
    text.insert(3,datatype)
    for d in report['definitions']:
        if selected is not None and d['name'] not in selected:continue
        variants={};failures=[]
        for event in sorted(by_class[d['eclass']]):
            result=engine.lower(event,d['name'].lstrip('$'))
            if result['status']=='lowered':
                key=result['egg']
                if key not in variants:variants[key]={**result,'occurrences':[]}
                variants[key]['occurrences'].append(event)
            else:failures.append(result)
        entry={'name':d['name'],'eclass':d['eclass'],'variants':list(variants.values()),'other_occurrences':failures}
        for k,v in enumerate(entry['variants']):
            v['egg']=v['egg'].replace(':name "'+d['name'].lstrip('$')+'"',':name "'+d['name'].lstrip('$')+'_v'+str(k)+'"')
            text+=['; @tier1-combined '+json.dumps({'comb':d['name'],'variant':k,'occurrences':v['occurrences'],'steps':len(v['steps']),'additional_entry_guards':v['guard_count']}),v['egg']]
        results.append(entry)
    summary={'comb_classes':len(results),'classes_with_lowering':sum(bool(x['variants']) for x in results),
        'rules':sum(len(x['variants']) for x in results),'lowered_occurrences':sum(len(v['occurrences']) for x in results for v in x['variants']),
        'other_statuses':dict(Counter(v['status'] for x in results for v in x['other_occurrences']))}
    (OUT/'combined.json').write_text(json.dumps({'scope':'Symbolic interpretation of the existing native tier-1 graph, with witnessed interface schemas. Guarded specializations; not a runtime speedup measurement.','selected_only':selected is not None,'summary':summary,'combinations':results},indent=2)+'\n')
    (OUT/'combined.egg').write_text('\n\n'.join(text)+'\n');print(summary)
if __name__=='__main__':
    import argparse
    p=argparse.ArgumentParser();p.add_argument('--selected',type=Path);a=p.parse_args()
    main(set(json.loads(a.selected.read_text())) if a.selected else None)

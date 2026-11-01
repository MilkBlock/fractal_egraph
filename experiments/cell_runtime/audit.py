"""Independent Python check of complete LHS bindings against stage-entry graphs.

This checks binding soundness, not completeness or all merge effects. Diagnostic
snapshots are only produced by --audit-stages and are not part of timed inference.
"""
import json
from pathlib import Path

def freeze(v):
 if 'key' in v:
  from eggfuse.ir.structural_key import key_token
  return v['sort'],key_token(v['key'])
 if 'eq' in v:return v['sort'],v['eq']
 if 'elements' in v:return v['sort'],tuple(sorted({freeze(x) for x in v['elements']}))
 return v['sort'],v['value']

def primitive(c,args):
 name=c['primitive'].removeprefix('bool-');out=c['output'];v=[x[1] for x in args]
 if name in ('=','!='):
  yes=(args[0]==args[1])==(name=='=');return (out,0 if out=='Unit' else int(yes)) if yes or out!='Unit' else None
 if name in ('<','<=','>','>='):
  yes={'<':v[0]<v[1],'<=':v[0]<=v[1],'>':v[0]>v[1],'>=':v[0]>=v[1]}[name]
  return (out,0 if out=='Unit' else int(yes)) if yes or out!='Unit' else None
 if name in ('+','-','*','/','%','min','max'):
  a,b=v
  if name in ('/','%'):
   if b==0 or (a==-(1<<63) and b==-1):return None
   div=(abs(a)//abs(b))*(-1 if (a<0)!=(b<0) else 1);n=div if name=='/' else a-div*b
  else:n={'+':lambda:a+b,'-':lambda:a-b,'*':lambda:a*b,'min':lambda:min(a,b),'max':lambda:max(a,b)}[name]()
  return (out,n) if -(1<<63)<=n<(1<<63) else None
 if name=='not':return out,int(not v[0])
 if name in ('and','or','xor','implies'):
  a,b=map(bool,v);return out,int({'and':a and b,'or':a or b,'xor':a!=b,'implies':not a or b}[name])
 if name in ('set-of','set-empty'):return out,tuple(sorted(set(args)))
 if name in ('set-contains','set-not-contains'):
  yes=(args[1] in v[0])==(name=='set-contains');return (out,0) if yes else None
 if name=='set-length':return out,len(v[0])
 if name in ('set-insert','set-remove'):
  xs=set(v[0]);xs.add(args[1]) if name=='set-insert' else xs.discard(args[1]);return out,tuple(sorted(xs))
 if name in ('set-union','set-intersect','set-diff'):
  a,b=map(set,v);xs={'set-union':a|b,'set-intersect':a&b,'set-diff':a-b}[name];return out,tuple(sorted(xs))
 raise AssertionError('unimplemented audit primitive '+str(c))

def check(program,runtime):
 runtime=Path(runtime);records=json.loads((runtime/'binding-audit.json').read_text());ends=json.loads((runtime/'stages.json').read_text());before=records['initial'];count=atoms=0
 assert len(records['runs'])==len(ends)
 for stage,(run,after) in enumerate(zip(records['runs'],ends)):
  facts={(r['table'],tuple(freeze(v) for v in r['values'])) for r in before['rows'] if not r['subsumed']}
  for binding in run['bindings']:
   rule=program['recipe']['rules'][binding['rule']];env={v['slot']:freeze(v['value']) for v in binding['values']}
   def arg(v):return env[v['slot']] if 'slot' in v else (v['sort']['name'],v['literal'])
   for atom in rule['query']:
    values=[arg(v) for v in atom['args']];call=atom['call']
    if 'table' in call:assert (call['table'],tuple(values)) in facts,(stage,binding['rule'],call,values)
    else:assert primitive(call,values[:-1])==values[-1],(stage,binding['rule'],call,values)
    atoms+=1
   count+=1
  before=after
 return {'kind':'independent complete-LHS stage-entry audit','bindings':count,'atoms':atoms,'runs':len(ends),'scope':'soundness of reported matches; not proof of completeness or whole effect semantics'}

"""Grounded-table graph comparison for the measured sum-bits endpoint (eq + scalar)."""
import json,sys
from pathlib import Path
schema,native,cell=map(Path,sys.argv[1:]); tables={x['name'] for x in json.load(open(schema))['tables']}
def term(x):
 if 'eq' in x:return (x['sort'],'eq',x['eq'])
 if 'value' in x:return (x['sort'],'lit',json.dumps(x['value'],sort_keys=True))
 raise ValueError(f'unsupported value {x!r}')
def rows(p):return [(r['table'],[term(v) for v in r['values']],r['subsumed']) for r in json.load(open(p))['rows'] if r['table'] in tables]
a,b=rows(native),rows(cell); idx={(t,tuple(v[:-1])):(v[-1],h) for t,v,h in a}; forward={};back={};proof={};checked=0
assert len(idx)==len(a)
def witness(x):return proof.get(x,repr(x))
def check():
 global checked
 todo=b[:]
 if len(a)!=len(b):return ['different cardinality',len(a),len(b)]
 while todo:
  rest=[]
  for t,v,h in todo:
   ins=v[:-1];dst=v[-1]
   if any(x[1]=='eq' and x not in forward for x in ins):rest.append((t,v,h));continue
   key=(t,tuple(forward.get(x,x) for x in ins));ex=t+'('+', '.join(witness(x) for x in ins)+')'
   if key not in idx:return ['missing grounded key',ex,str(key)]
   other,nh=idx[key]
   if nh!=h:return ['visibility',ex,h,nh]
   if dst[1]=='lit':
    if dst!=other:return ['scalar mismatch',ex,str(dst),str(other)]
   else:
    if dst in forward and forward[dst]!=other:return ['equality split',ex,witness(dst),str(other),str(forward[dst])]
    if other in back and back[other]!=dst:return ['equality collapse',ex,witness(back[other]),str(dst),str(back[other])]
    forward[dst]=other;back[other]=dst;proof.setdefault(dst,ex[:300])
   checked+=1
  if len(rest)==len(todo):return ['unresolved cyclic component',len(rest)]
  todo=rest
 return ['grounded rows, equality and visibility agree']
print(json.dumps({'result':check(),'checked_before_stop':checked,'class_pairs':len(forward),'native_rows':len(a),'cell_rows':len(b)},indent=2))

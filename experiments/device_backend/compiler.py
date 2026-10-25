"""Lower native typed core rules once; no textual .egg parsing or CPU matching."""
from pathlib import Path
import json
HERE=Path(__file__).resolve().parent

def boolean_expr(call, args):
 op=call['primitive']
 if any(t!='bool' for t in call['inputs']):return None
 if op=='not' and len(args)==1:return f'(!({args[0]}))'
 if op=='or':return '('+' || '.join(args or ['false'])+')'
 if len(args)==2 and op in ('and','xor','=>'):
  a,b=args
  return {'and':f'({a} && {b})','xor':f'({a} != {b})','=>':f'(!({a}) || {b})'}[op]
 return None

INTEGER_OPS={'+':0,'-':1,'*':2,'/':3,'%':4,'min':5,'max':6,'bool-=':7,'bool-<':8,'bool->':9,'bool-<=':10,'bool->=':11}
def integer_op(c):
 return INTEGER_OPS.get(c['primitive']) if c['inputs']==['i64','i64'] else None

def compile_program(p,capacity=4096,match_capacity=16384):
 if p['schema']!='egglog-device-core/v1':raise ValueError('program schema')
 strings=set()
 def collect(x):
  if isinstance(x,dict):
   if x.get('sort')=='String' and 'value' in x:strings.add(x['value'])
   if isinstance(x.get('sort'),dict) and x['sort'].get('kind')=='String' and 'literal' in x:strings.add(x['literal'])
   for v in x.values():collect(v)
  elif isinstance(x,list):
   for v in x:collect(v)
 collect(p);strings=sorted(strings);string_ids={s:i for i,s in enumerate(strings)}
 ts=p['tables'];ids={t['name']:i for i,t in enumerate(ts)};cols=max([len(t['schema']) for t in ts]+[1]);width=[];eq=[];kind=[];policy=[]
 for t in ts:
  width.append(len(t['schema']));eq.extend([int(s['kind']=='eq') for s in t['schema']]+[0]*(cols-len(t['schema'])))
  k={'constructor':0,'function':2}[t['kind']];kind.append(k)
  if k==0 and t['schema'][-1]['kind']!='eq':raise ValueError('device constructors require equality-sort output')
  merge=t['merge'];pol=3 if k==0 else 0
  if merge is not None:
   if merge=='old':pol=4
   elif merge=='new':pol=5
   elif merge in ('(min old new)','(min new old)') and t['schema'][-1]['kind']=='i64':pol=1
   elif merge in ('(max old new)','(max new old)') and t['schema'][-1]['kind']=='i64':pol=2
   elif t.get('merge_expr') is not None:pol=6
   else:raise ValueError('unsupported custom merge '+merge)
  policy.append(pol)
 anchors=sorted({(x['sort'],x['eq']) for row in p['initial'] for x in row['values'] if 'eq' in x});identity={k:i+1 for i,k in enumerate(anchors)}
 flat=[]
 for i,row in enumerate(p['initial']):
  vs=[identity[(x['sort'],x['eq'])] if 'eq' in x else (string_ids[x['value']] if x['sort']=='String' else x['value']) for x in row['values']]
  flat += [ids[row['table']],int(row['subsumed']),1,i+1,*vs,*([0]*(cols-len(vs)))]
 if capacity<1 or capacity<len(p['initial']) or match_capacity<1:raise ValueError('invalid initial capacity')
 idcap=max(capacity*2+1,len(anchors)+capacity+1)
 rules=[];keys={};sets=[]
 for rs in p['rulesets']:
  use=[]
  for r in rs['rules']:
   key=(r.get('source_set',rs['name']),r['name'])
   if key not in keys:keys[key]=len(rules);rules.append(r)
   elif rules[keys[key]]!=r:raise ValueError('rule identity changed')
   use.append(keys[key])
  if len(set(use))!=len(use):raise ValueError('duplicate ruleset membership unsupported')
  sets.append(use)
 allvars=[];queries=[];actions=[]
 for rid,r in enumerate(rules):
  slots={};types={}
  def var(name,sort):
   if name not in slots:slots[name]=len(slots);types[name]=sort
   elif types[name]!=sort:raise ValueError('variable changes type')
   return slots[name]
  def operand(x):
   if 'var' in x:return f'v[{var(x["var"],x["sort"]["name"])}]'
   n=string_ids[x['literal']] if x['sort']['kind']=='String' else x['literal']
   if not -(1<<63)<=n<(1<<63):raise ValueError('integer range')
   return '(-9223372036854775807L-1L)' if n==-(1<<63) else str(n)+'L'
  def bind(x,value,known):
   if 'literal' in x:return [f'if({value}!={operand(x)})continue;']
   name=x['var'];dst=operand(x)
   if name in known:return [f'if({dst}!={value})continue;']
   known.add(name);return [f'{dst}={value};']
  pending=list(r['body']);known=set();lines=[];depth=0;stamps=[]
  def ready(a):return 'table' in a['call'] or all('literal' in x or x['var'] in known for x in a['args'][:-1])
  while pending:
   at=next((i for i,a in enumerate(pending) if ready(a)),None)
   if at is None:raise ValueError('ungrounded primitive')
   a=pending.pop(at);c=a['call']
   if 'table' in c:
    t=ids[c['table']]
    if len(a['args'])!=width[t]:raise ValueError('arity')
    iv=f'i{depth}';depth+=1;lines.append(f'for(uint {iv}=0;{iv}<oldCount;{iv}++){{')
    lines.append(f'if(old[{iv}*STRIDE]!={t} || old[{iv}*STRIDE+1]!=0)continue;');stamps.append(f'uint(old[{iv}*STRIDE+2])')
    for col,x in enumerate(a['args']):lines+=bind(x,f'old[{iv}*STRIDE+4+{col}]',known)
   else:
    op=c['primitive'];args=a['args']
    be=boolean_expr(c,[operand(x) for x in args[:-1]])
    if be is not None:
     lines+=bind(args[-1],be,known);continue
    if op=='guard' and c['inputs']==['bool']:
     lines.append(f'if(!({operand(args[0])}))continue;');lines+=bind(args[-1],'0L',known);continue
    if len(args)!=3:raise ValueError('only binary primitives supported')
    av,bv=map(operand,args[:2]);numeric=c['inputs']==['i64','i64']
    if op in ('<','>','<=','>=','!=') and (numeric or op=='!='):
     lines.append(f'if(!({av}{op}{bv}))continue;');lines+=bind(args[-1],'0L',known)
    elif integer_op(c) is not None:
     code=integer_op(c)
     lines.append(f'if(!integer_valid({code},{av},{bv}))continue;')
     lines+=bind(args[-1],f'integer_eval({code},{av},{bv})',known)
    else:raise ValueError('unsupported LHS primitive '+op)
  stamp='0u'
  for s in stamps:stamp=f'max({stamp},{s})'
  condition=(f'{stamp}>watermarks[{rid}]' if stamps else f'watermarks[{rid}]==0') if r['seminaive'] else 'true'
  lines.append(f'if({condition}){{ if(m.matched>=MATCHCAP){{m.error=2;return;}} uint q=m.matched++*MSTRIDE;matches[q]={rid};for(uint c=0;c<VARS;c++)matches[q+1+c]=v[c];}}')
  lines+=['}']*depth
  queries.append('\n'.join(lines))
  out=[]
  def args_setup(xs):
   for i,x in enumerate(xs):
    if 'var' in x and x['var'] not in known:raise ValueError('ungrounded RHS variable')
    out.append(f'args[{i}]={operand(x)};')
  for a in r['head']:
   op=a['op']
   if op=='copy':out.append(f'v[{var(a["dst"],a["sort"]["name"])}]={operand(a["arg"])};');known.add(a['dst'])
   elif op=='panic':out.append('m.error=8;break;')
   elif op=='union':
    x,y=a['args'];
    if x['sort']['kind']!='eq' or x['sort']!=y['sort']:raise ValueError('union requires one equality sort')
    out.append(f'unite(p,{operand(x)},{operand(y)},m);')
   elif op in ('let','set','subsume'):
    c=a['call'];args_setup(a['args'])
    if 'primitive' in c:
     if op!='let':raise ValueError('unsupported RHS primitive action')
     expr=boolean_expr(c,[f'args[{i}]' for i in range(len(a['args']))])
     if expr is None:
      code=integer_op(c)
      if code is None:raise ValueError('unsupported RHS primitive '+c['primitive'])
      out.append(f'if(!integer_valid({code},args[0],args[1])){{m.error=5;break;}}')
      expr=f'integer_eval({code},args[0],args[1])'
    else:
     t=ids[c['table']]
     if op=='subsume':
      if not ts[t]['can_subsume'] or kind[t]!=0:raise ValueError('unsupported custom-view subsume')
      out.append(f'hide_row(r,p,fp,m,{t},args);');continue
     if op=='set':
      # Native core folds union(constructor(...), existing) into Set.
      out.append(f'write_row(r,p,fp,m,{t},args,{operand(a["value"])},false);');continue
     if kind[t]==2 and not ts[t]['internal']:raise ValueError('RHS custom function read not yet supported')
     expr=f'write_row(r,p,fp,m,{t},args,0,true)'
    out.append(f'v[{var(a["dst"],a["sort"]["name"])}]={expr};');known.add(a['dst'])
   else:raise ValueError('unsupported action '+op)
  actions.append(f'case {rid}:{{'+ '\n'.join(out)+'break;}');allvars.append(slots)
 nv=max([len(s) for s in allvars]+[1])
 if max(capacity*(cols+4),match_capacity*(nv+1),idcap)>=(1<<32):raise ValueError('device address space exhausted')
 prefix=f'#define COLS {cols}\n#define STRIDE {cols+4}\n#define VARS {nv}\n#define MSTRIDE {nv+1}\n#define CAPACITY {capacity}\n#define MATCHCAP {match_capacity}\n#define IDCAP {idcap}\n'
 for name,data in [('WIDTH',width),('EQ',eq),('KIND',kind),('POLICY',policy)]:prefix+='constant uint '+name+'[]={'+','.join(map(str,data or [0]))+'};\n'
 merge_cases=[]
 for tid,t in enumerate(ts):
  if policy[tid]!=6:continue
  lines=[];counter=[0]
  def lower(e):
   if 'var' in e:
    if e['var'] not in ('old','new'):raise ValueError('unknown merge variable')
    return 'a' if e['var']=='old' else 'b'
   if 'literal' in e:
    v=string_ids[e['literal']] if e['sort']['kind']=='String' else e['literal']
    return '(-9223372036854775807L-1L)' if v==-(1<<63) else str(v)+'L'
   args=[lower(x) for x in e['args']];c=e['call'];n=counter[0];counter[0]+=1;name=f'mv{n}'
   if 'table' in c:
    target=ids[c['table']]
    if kind[target]!=0:raise ValueError('merge function reads unsupported')
    lines.append(f'long ma{n}[COLS]={{'+','.join(args or ['0'])+'};')
    expr=f'ensure_constructor(r,p,fp,m,{target},ma{n})'
   else:
    code=integer_op(c);expr=boolean_expr(c,args)
    if code is not None:
     lines.append(f'if(!integer_valid({code},{args[0]},{args[1]})){{m.error=5;return a;}}')
     expr=f'integer_eval({code},{args[0]},{args[1]})'
    if expr is None:raise ValueError('unsupported merge primitive '+c['primitive'])
   lines.append(f'long {name}={expr};if(m.error)return a;');return name
  value=lower(t['merge_expr']);merge_cases.append(f'case {tid}:{{'+''.join(lines)+f'return {value};'+'}')
 # Metal scalar types are available after the include, before generated schema.
 source=(HERE/'kernel.metal').read_text().replace('// MERGE_BODY',''.join(merge_cases)).replace('// Generated schema/limits are inserted before this code.',prefix)
 query=[];wm=[]
 for si,use in enumerate(sets):
  query.append(f'if(set=={si}){{'+''.join('{long v[VARS]={0};do{'+queries[r]+'}while(false);}' for r in use)+'}')
  wm.append(f'if(set=={si}){{'+''.join(f'watermarks[{r}]=previous;' for r in use)+'}')
 source=source.replace('// MATCH_BODY','\n'.join(query)).replace('// ACTION_BODY','\n'.join(actions)).replace('// WATERMARK_BODY','\n'.join(wm))
 return {'schema':'egglog-device-storage/v1','shader':source,'tables':ts,'strings':strings,'initial_rows':flat,'anchors':anchors,'sets':[s['name'] for s in p['rulesets']],'schedule':p['schedule'],'assertion_tables':p.get('assertion_tables',[]),'capacity':capacity,'match_capacity':match_capacity,'id_capacity':idcap,'stride':cols+4,'match_stride':nv+1,'rules':len(rules),'meta':[len(p['initial']),len(anchors)+1,1,0,0,0,0,0,len(p['initial'])+1]}

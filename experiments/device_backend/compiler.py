"""Lower native typed core rules once; no textual .egg parsing or CPU matching."""
from pathlib import Path
import json,re
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

def compile_program(p,capacity=4096,match_capacity=16384,defer_unions=True,incremental_queries=True,delta_queries=True,parallel_rebuild=True,parallel_prepare=True,parallel_match=True,indexed_subsume=True,parallel_merge_index=True,all_index_columns=False):
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
  width.append(len(t['schema']));eq.extend([(2 if s['kind']=='set' else int(s['kind']=='eq')) for s in t['schema']]+[0]*(cols-len(t['schema'])))
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
 set_sorts=sorted({s['name'] for t in ts for s in t['schema'] if s['kind']=='set'});set_ids={name:i for i,name in enumerate(set_sorts)}
 def atoms(x):
  if 'elements' in x:
   for e in x['elements']:yield from atoms(e)
  else:yield x
 anchors=sorted({(x['sort'],x['eq']) for row in p['initial'] for v in row['values'] for x in atoms(v) if 'eq' in x});identity={k:i+1 for i,k in enumerate(anchors)}
 arena=[2,1];intern={}
 def encode(x):
  if 'eq' in x:return identity[(x['sort'],x['eq'])]
  if 'elements' in x:
   es=tuple(sorted({encode(e) for e in x['elements']}));key=(x['sort'],es)
   if key not in intern:
    intern[key]=len(arena);arena.extend([len(es),len(es),set_ids[x['sort']],1,*es]);arena[0]=len(arena)
   return intern[key]
  return string_ids[x['value']] if x['sort']=='String' else x['value']
 flat=[]
 for i,row in enumerate(p['initial']):
  vs=[encode(x) for x in row['values']]
  flat += [ids[row['table']],int(row['subsumed']),1,i+1,*vs,*([0]*(cols-len(vs)))]
 arena_words=max(len(arena),capacity*16) if set_sorts else 2
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
 def set_expr(c,args):
  ops={'set-empty':0,'set-insert':1,'set-union':2,'set-intersect':3,'set-length':4}
  if c['primitive'] not in ops:return None
  name=c['output'] if c['output'] in set_ids else c['inputs'][0]
  if name not in set_ids:raise ValueError('unknown set sort')
  values=(args+['0','0'])[:2]
  return f'set_apply(r,p,m,{ops[c["primitive"]]},{set_ids[name]},{values[0]},{values[1]})'
 # Native RHS union inserts into the UF mutation table; it does not eagerly
 # canonicalize subsequent RHS arguments. Flush after the action phase.
 deferred=[int(defer_unions) for _ in sets]
 def read_only_query(rule):
  for a in rule['body']:
   c=a['call']
   if c.get('primitive','').startswith('set-') and c['primitive']!='set-length':return False
  return True
 parallel_sets=[int(parallel_match and all(read_only_query(rules[r]) for r in use)) for use in sets]
 serial_use={r for si,use in enumerate(sets) if not parallel_sets[si] for r in use}
 index_columns=[set() for _ in ts]
 union_cap=max(capacity,match_capacity*2)
 allvars=[];queries=[];action_code=[];action_starts=[];query_widths=[]
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
  atoms=[dict(a) for a in r['body']];table_atoms=[]
  for a in atoms:
   for x in a['args']:
    if 'var' in x:var(x['var'],x['sort']['name'])
   if 'table' in a['call']:a['_table_atom']=len(table_atoms);table_atoms.append(a)
  query_widths.append(len(slots));variants=[]
  pivots=range(len(table_atoms)) if delta_queries and r['seminaive'] and table_atoms else [None]
  for pivot in pivots:
   pending=list(atoms);known=set();lines=[];depth=0;stamps=[];pivot_pending=pivot is not None;seeded=False;seed_row="0";seed_time="0"
   def ready(a):return 'table' in a['call'] or all('literal' in x or x['var'] in known for x in a['args'][:-1])
   while pending:
    candidates=[i for i,a in enumerate(pending) if ready(a)]
    # Consume grounded filters first, then connected table atoms. This avoids
    # enumerating unrelated type/shape tables before their binding is available.
    def rank(i):
     a=pending[i]
     return (3 if 'primitive' in a['call'] else (2 if pivot_pending and a.get('_table_atom')==pivot else 0),sum('literal' in x or x.get('var') in known for x in a['args']),-i)
    at=max(candidates,key=rank) if candidates else None
    if at is None:raise ValueError('ungrounded primitive')
    a=pending.pop(at);c=a['call']
    if 'table' in c:
     atom_index=a['_table_atom'];pivot_pending=pivot_pending and atom_index!=pivot
     t=ids[c['table']]
     if len(a['args'])!=width[t]:raise ValueError('arity')
     bound=[i for i,x in enumerate(a['args']) if 'literal' in x or x['var'] in known]
     iv=f'i{depth}';entry=f'q{depth}';depth+=1
     if all(c in bound for c in range(width[t]-1)):
      # A complete function/constructor key identifies at most one visible row.
      # Looking up one column here would unnecessarily scan its whole bucket.
      values=','.join(operand(x) for x in a['args'][:-1]) or '0'
      lines.append(f'long key{depth}[COLS]={{'+values+'};')
      lines.append(f'long found{depth}=query_lookup(r,{t},key{depth});')
      lines.append(f'for(long once{depth}=found{depth};once{depth}>=0;once{depth}=-1){{uint {iv}=uint(once{depth});')
     else:
      column=bound[-1] if bound else cols
      if seeded or rid in serial_use:
       index_columns[t].add(column)
       if pivot is not None and atom_index==pivot:index_columns[t].add(cols+1)
      head=f'qheads[query_hash({t},{column},{operand(a["args"][column])})]' if bound else f'qheads[HASH_CAP+{t}]'
      if pivot is not None and atom_index==pivot:
       timestamp_var=f'delta_ts{depth}';lines.append(f'uint {timestamp_var}=watermarks[{rid}]+1;')
       begin=f'watermarks[{rid}]?delta_first({t},m.epoch,{timestamp_var},old,qheads,qlinks):{head}'
       following=f'watermarks[{rid}]?delta_next({t},m.epoch,{entry},{timestamp_var},old,qheads,qlinks):qlinks[{entry}]'
       selected=f'(watermarks[{rid}]?TIME_COLUMN:{column})'
      else:begin=head;following=f'qlinks[{entry}]';selected=str(column)
      if not seeded:
       begin=f'(QUERY_PARALLEL[set]?(lane<m.count?lane*QSTRIDE+{selected}:0xffffffff):({begin}))'
       following=f'(QUERY_PARALLEL[set]?0xffffffff:({following}))'
       seed_row=iv;seed_time=f'(watermarks[{rid}]?uint(old[{iv}*STRIDE+2]):0u)' if pivot is not None and atom_index==pivot else '0u';seeded=True
      lines.append(f'for(uint {entry}={begin};{entry}!=0xffffffff;{entry}={following}){{')
      lines.append(f'if({entry}%QSTRIDE!={selected})continue;uint {iv}={entry}/QSTRIDE;')
     lines.append(f'if(old[{iv}*STRIDE]!={t} || old[{iv}*STRIDE+1]!=0)continue;');stamps.append(f'uint(old[{iv}*STRIDE+2])')
     if pivot is not None and atom_index<=pivot:
      op='<=' if atom_index==pivot else '>'
      lines.append(f'if(uint(old[{iv}*STRIDE+2]){op}watermarks[{rid}])continue;')
     for col,x in enumerate(a['args']):lines+=bind(x,f'old[{iv}*STRIDE+4+{col}]',known)
    else:
     op=c['primitive'];args=a['args']
     be=f'r[ROW_WORDS+{operand(args[0])}+1]' if op=='set-length' else set_expr(c,[operand(x) for x in args[:-1]]) or boolean_expr(c,[operand(x) for x in args[:-1]])
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
   variant=0 if pivot is None else pivot
   lines.append(f'if({condition})'+'{ uint item; if(QUERY_PARALLEL[set])item=atomic_fetch_add_explicit((device atomic_uint*)&m.matched,1u,memory_order_relaxed);else item=m.matched++;if(item>=MATCHCAP){atomic_store_explicit((device atomic_uint*)&m.error,2u,memory_order_relaxed);return;}uint q=item*MSTRIDE;'+f'matches[q]={rid};'+f'for(uint c=0;c<VARS;c++)matches[q+1+c]=v[c];matches[q+1+VARS]=rank;matches[q+2+VARS]={variant};matches[q+3+VARS]={seed_time};matches[q+4+VARS]={seed_row};matches[q+5+VARS]=sequence++;'+'}')
   lines+=['}']*depth
   if pivot is not None:gate=f'qheads[2*QBUCKETS+1+{ids[table_atoms[pivot]["call"]["table"]]}]>watermarks[{rid}]'
   elif table_atoms and r['seminaive']:
    latest='0u'
    for atom in table_atoms:latest=f'max({latest},qheads[2*QBUCKETS+1+{ids[atom["call"]["table"]]}])'
    gate=f'{latest}>watermarks[{rid}]'
   else:gate='true'
   if not seeded:gate=f'({gate})&&(!QUERY_PARALLEL[set]||lane==0)'
   variants.append(f'if({gate})'+'{uint sequence=0;long v[VARS]={0};do{'+ '\n'.join(lines)+'}while(false);}')
  queries.append("\n".join(variants))
  action_starts.append(len(action_code))
  def instruction(op,dst=0,sub=0,k=0,value=0):action_code.extend([op,dst,sub,k,value])
  def input_value(x):
   if 'var' in x:
    if x['var'] not in known:raise ValueError('ungrounded RHS variable')
    return 1,var(x['var'],x['sort']['name'])
   return 0,(string_ids[x['literal']] if x['sort']['kind']=='String' else x['literal'])
  def load(i,x):
   k,value=input_value(x);instruction(0,i,0,k,value)
  for a in r['head']:
   op=a['op']
   if op=='copy':
    k,value=input_value(a['arg']);instruction(1,var(a['dst'],a['sort']['name']),0,k,value);known.add(a['dst'])
   elif op=='panic':instruction(9)
   elif op=='union':
    x,y=a['args']
    if x['sort']['kind']!='eq' or x['sort']!=y['sort']:raise ValueError('union requires one equality sort')
    load(0,x);load(1,y);instruction(2)
   elif op in ('let','set','subsume'):
    c=a['call']
    if len(a['args'])>cols:raise ValueError('action argument capacity exceeded')
    for i,x in enumerate(a['args']):load(i,x)
    if 'primitive' in c:
     if op!='let':raise ValueError('unsupported RHS primitive action')
     dst=var(a['dst'],a['sort']['name']);code=integer_op(c)
     if code is not None:instruction(6,dst,code)
     elif boolean_expr(c,['x']*len(a['args'])) is not None:
      instruction(7,dst,{'not':0,'and':1,'xor':2,'=>':3,'or':4}[c['primitive']],0,len(a['args']))
     elif set_expr(c,['x']*len(a['args'])) is not None:
      name=c['output'] if c['output'] in set_ids else c['inputs'][0]
      instruction(8,dst,{'set-empty':0,'set-insert':1,'set-union':2,'set-intersect':3,'set-length':4}[c['primitive']],0,set_ids[name])
     else:raise ValueError('unsupported RHS primitive '+c['primitive'])
     known.add(a['dst'])
    else:
     t=ids[c['table']]
     if op=='subsume':
      if not ts[t]['can_subsume'] or kind[t]!=0:raise ValueError('unsupported custom-view subsume')
      instruction(5,0,t)
     elif op=='set':load(width[t]-1,a['value']);instruction(4,0,t)
     else:
      if kind[t]==2 and not ts[t]['internal']:raise ValueError('RHS custom function read not yet supported')
      instruction(3,var(a['dst'],a['sort']['name']),t);known.add(a['dst'])
   else:raise ValueError('unsupported action '+op)
  allvars.append(slots)
 action_starts.append(len(action_code))
 nv=max(query_widths+[1])
 if max(capacity*(cols+4),match_capacity*(nv+6),idcap)>=(1<<32):raise ValueError('device address space exhausted')
 prefix=f'#define COLS {cols}\n#define STRIDE {cols+4}\n#define VARS {nv}\n#define MSTRIDE {nv+6}\n#define CAPACITY {capacity}\n#define MATCHCAP {match_capacity}\n#define IDCAP {idcap}\n'
 hash_cap=1
 while hash_cap<capacity*4:hash_cap*=2
 radix_bits=(hash_cap+len(ts)-1).bit_length()
 prefix+=f'#define PARALLEL_MERGE_INDEX {int(parallel_merge_index)}\n'
 prefix+=f'#define INDEXED_SUBSUME {int(indexed_subsume)}\n'
 prefix+=f'#define PARALLEL_PREPARE {int(parallel_prepare)}\n#define QUERY_RADIX_BITS {radix_bits}\n'
 prefix+=f'#define PARALLEL_REBUILD {int(parallel_rebuild)}\n'
 prefix+=f'#define UNION_CAP {union_cap}\n'
 prefix+=f'#define INCREMENTAL_QUERY {int(incremental_queries)}\n#define QUERY_DIRTY (INDEX_OFFSET+HASH_CAP+2)\n#define QBUCKETS (HASH_CAP+TABLE_COUNT)\n'
 prefix+=f'#define QSTRIDE {cols+2}\n#define TIME_COLUMN {cols+1}\n#define TABLE_COUNT {len(ts)}\n'
 prefix+=f'#define HASH_CAP {hash_cap}\n#define INDEX_OFFSET {capacity*(cols+4)+arena_words}\n'
 prefix+=f'#define ROW_WORDS {capacity*(cols+4)}\n#define SET_WORDS {arena_words}\n'
 set_start=[0];set_rules=[]
 for use in sets:set_rules.extend(use);set_start.append(len(set_rules))
 def merge_has_table(e):
  return isinstance(e,dict) and ('table' in e.get('call',{}) or any(merge_has_table(x) for x in e.get('args',[])))
 merge_reads=[int(merge_has_table(t.get('merge_expr'))) for t in ts]
 if all_index_columns:index_columns=[set(range(width[i]))|{cols,cols+1} for i in range(len(ts))]
 masks=[int(c in index_columns[t]) for t in range(len(ts)) for c in range(cols+2)]
 counts=[len(s) for s in index_columns]
 for name,data in [('INDEX_COLUMN',masks),('INDEX_COUNT',counts),('WIDTH',width),('EQ',eq),('KIND',kind),('POLICY',policy),('MERGE_INDEX',merge_reads),('ACTION_START',action_starts),('QUERY_WIDTH',query_widths),('DEFER_UNIONS',deferred),('QUERY_PARALLEL',parallel_sets),('SET_START',set_start),('SET_RULES',set_rules)]:prefix+='constant uint '+name+'[]={'+','.join(map(str,data or [0]))+'};\n'
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
 source=(HERE/'kernel.metal').read_text().replace('// MATCH_SORT',(HERE/'match_sort.metal').read_text()).replace('// PARALLEL_QUERY',(HERE/'query_parallel.metal').read_text()).replace('// PARALLEL_REBUILD',(HERE/'rebuild_parallel.metal').read_text()).replace('// QUERY_INDEX',(HERE/'query_index.metal').read_text()).replace('// ROW_INDEX',(HERE/'row_index.metal').read_text()).replace('// DEVICE_ACTIONS',(HERE/'actions.metal').read_text()).replace('// SET_BODY',(HERE/'sets.metal').read_text()).replace('// MERGE_BODY',''.join(merge_cases)).replace('// Generated schema/limits are inserted before this code.',prefix)
 kernel_start=source.index('// Correctness-first reference executor.')
 common,template=source[:kernel_start],source[kernel_start:]
 signature=common.split('#define KERNEL_ARGS ',1)[1].splitlines()[0]
 signature=re.sub(r'\[\[.*?\]\]','',signature)+',uint rank'
 call_args='r,old,p,fp,m,matches,watermarks,set,code,registers,qheads,qlinks,control,match_order,lane'
 query_functions='\n'.join(f'__attribute__((noinline)) void query_{rid}({signature})'+'{'+q+'}' for rid,q in enumerate(queries))
 kernels=[];kernel_names=[]
 for si,use in enumerate(sets):
  name=f'step_{si}';kernel_names.append(name)
  calls=''.join(f'query_{r}({call_args},{rank});if(atomic_load_explicit((device atomic_uint*)&m.error,memory_order_relaxed))return;' for rank,r in enumerate(use))
  kernels.append(template.replace('kernel void step(',f'kernel void {name}(').replace('// MATCH_BODY',calls))
 source=common+query_functions+'\n'.join(kernels)
 return {'schema':'egglog-device-storage/v1','parallel_rebuild':parallel_rebuild,'parallel_match_sets':parallel_sets,'indexed_subsume':indexed_subsume,'parallel_merge_index':parallel_merge_index,'index_columns':{t['name']:sorted(index_columns[i]) for i,t in enumerate(ts)},'match_order_capacity':max(256,1<<(match_capacity-1).bit_length()),'parallel_prepare':parallel_prepare,'query_radix_bits':radix_bits,'query_sort_words':capacity*(cols+2),'hash_capacity':hash_cap,'shader':source,'kernel_names':kernel_names,'action_code':action_code,'register_count':max([len(x) for x in allvars]+[1]),'set_rule_ids':sets,'query_dirty_offset':capacity*(cols+4)+arena_words+hash_cap+2,'tables':ts,'strings':strings,'initial_rows':flat,'initial_sets':arena,'set_words':arena_words,'index_words':hash_cap+3,'incremental_queries':incremental_queries,'delta_queries':delta_queries,'index_counter_offset':capacity*(cols+4)+arena_words+hash_cap+1,'union_words':2+union_cap*2,'deferred_rulesets':[s['name'] for i,s in enumerate(p['rulesets']) if deferred[i]],'query_head_words':2*(hash_cap+len(ts))+1+len(ts),'query_link_words':capacity*(cols+2),'anchors':anchors,'sets':[s['name'] for s in p['rulesets']],'schedule':p['schedule'],'assertion_tables':p.get('assertion_tables',[]),'capacity':capacity,'match_capacity':match_capacity,'workspace_words':max(match_capacity*(nv+6),capacity*(cols+4)),'id_capacity':idcap,'stride':cols+4,'match_stride':nv+6,'rules':len(rules),'meta':[len(p['initial']),len(anchors)+1,1,0,0,0,0,0,len(p['initial'])+1]}

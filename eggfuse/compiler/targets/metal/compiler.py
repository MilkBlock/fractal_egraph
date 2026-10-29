"""Lower native typed core rules once; no textual .egg parsing or CPU matching."""
from pathlib import Path
import json,re
from .batch_rhs import lower as lower_batch_rhs
from .matrix_sat import lower as lower_matrix_sat
from .matrix_comparison import lower as lower_matrix_comparison
from .joint_polyspine import lower as lower_joint_polyspine
from .query_contexts import split_query, HEADER as CONTEXT_HEADER
from .context_ridge import feature_layout, lower as lower_ridge
HERE=Path(__file__).resolve().parents[4]/'engine/metal'

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

def encode_initial(p,cols,string_ids,native_order=False):
 """Typed instance data only; does not recompile rules for another binding."""
 ts=p['tables'];ids={t['name']:i for i,t in enumerate(ts)}
 set_ids={name:i for i,name in enumerate(sorted({s['name'] for t in ts for s in t['schema'] if s['kind']=='set'}))}
 def atoms(x):
  if 'elements' in x:
   for e in x['elements']:yield from atoms(e)
  else:yield x
 keys={(x['sort'],x['eq']) for row in p['initial'] for v in row['values'] for x in atoms(v) if 'eq' in x}
 def order(k):
  if not native_order:return k
  m=re.fullmatch(r'Value\((\d+)\)',k[1])
  if not m:raise ValueError('native identity ordering needs an exported Value ordinal')
  return (k[0],int(m[1]))
 anchors=sorted(keys,key=order);identity={k:i+1 for i,k in enumerate(anchors)}
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
 return anchors,arena,flat

def compile_program(p,capacity=4096,match_capacity=16384,defer_unions=True,incremental_queries=True,delta_queries=True,parallel_rebuild=True,parallel_prepare=True,parallel_match=True,indexed_subsume=True,parallel_merge_index=True,all_index_columns=False,radix_digit_bits=4,seed_dispatch=True,noop_preflight=True,direct_actions=True,query_patch=True,effect_dedup=False,early_seed_guards=False,lhs_context_reuse=True,rebuild_batch=1,parallel_rhs=False,split_matches=False,context_model=None,context_training=False,merge_target_index=False,batch_rhs=False,matrix_saturate=False,joint_polyspine=False,polyspine_model=None,polyspine_training=False,polyspine_full_refresh=False,polyspine_refresh_audit=False,stage_contexts=False,block_merge=False,block_merge_audit=False,cell_program=None,scheduled_matrix=False,marker_program=None,region_program=None,resident_contexts=False,lazy_contexts=False):
 marker_names=set()
 marker_shortcuts={}
 if marker_program is not None:
  from eggfuse.compiler.marker_ir import validate,completion_shortcuts
  marker_names=validate(marker_program,p)
  marker_shortcuts=completion_shortcuts(marker_program,p)
 if region_program is not None:
  from eggfuse.ir.regions import validate_plan as verify_regions
  from eggfuse.ir.recipe import build as recipe_ir
  if region_program['marker_names']!=sorted(marker_names):raise ValueError('region marker environment differs')
  verify_regions(region_program,recipe_ir({k:v for k,v in p.items() if k!='cell_input_constants'},region_program['scope']))
  if split_matches or joint_polyspine or scheduled_matrix or not delta_queries:
   raise ValueError('region program requires the shared GPU control executor')
  if parallel_match and not resident_contexts:raise ValueError('parallel regions require resident context preparation and stable binding publication')
  # Source markers remain ordinary facts. Their consumers use the same exact
  # cached queries; specialized completion shortcuts are not a second authority.
  marker_shortcuts={}
 batch_enabled=region_program is not None and 'batch_program' in region_program
 completed_rules=set()
 if batch_enabled:
  from eggfuse.ir.recipe import build as recipe_ir
  certificate_core=recipe_ir({k:v for k,v in p.items() if k!='cell_input_constants'},region_program['scope'])
  for t in region_program['batch_program']['templates']:
   for step in t['steps'][1:]:
    rr=certificate_core['rules'][step['recipe_rule']]
    atoms=[a for a in rr['query'] if 'table' in a['call']]
    from eggfuse.ir.batches import certifiable
    if certifiable(rr,p['tables']):completed_rules.add(step['rule'])
 region_source=''
 scheduled_info=None
 if scheduled_matrix:
  from .scheduled_matrix import prepare
  p,scheduled_info=prepare(p);matrix_saturate=True
 if cell_program is not None and not (joint_polyspine and block_merge):raise ValueError('Cell instructions require joint execution and block merge')
 if (context_model is not None or context_training) and not split_matches:raise ValueError('context ridge requires split matching')
 if rebuild_batch<1:raise ValueError('rebuild batch must be positive')
 if radix_digit_bits not in (1,4):raise ValueError('radix digit bits must be 1 or 4')
 if p['schema']!='egglog-device-core/v1':raise ValueError('program schema')
 for table in p['tables']:
  for sort in table['schema']:
   if sort['kind'] not in ('eq','i64','Unit','String','bool','f64','set'):
    raise ValueError('GPU has no physical lowering for sort '+sort['name']+' in '+table['name'])
 strings=set()
 def collect(x):
  if isinstance(x,dict):
   if x.get('sort')=='String' and 'value' in x:strings.add(x['value'])
   if isinstance(x.get('sort'),dict) and x['sort'].get('kind')=='String' and 'literal' in x:strings.add(x['literal'])
   for v in x.values():collect(v)
  elif isinstance(x,list):
   for v in x:collect(v)
 collect(p);strings=sorted(strings);string_ids={s:i for i,s in enumerate(strings)}
 # Temporary action arguments can be wider than every physical table.
 def scratch_arity(x):
  if isinstance(x,dict):return max([len(x.get('args',[]))]+[scratch_arity(v) for v in x.values()])
  if isinstance(x,list):return max([scratch_arity(v) for v in x]+[0])
  return 0
 ts=p['tables'];ids={t['name']:i for i,t in enumerate(ts)};cols=max([len(t['schema']) for t in ts]+[1,scratch_arity(p['rulesets']),scratch_arity(ts)]);width=[];eq=[];kind=[];policy=[]
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
 anchors,arena,flat=encode_initial(p,cols,string_ids)
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
 context_reuses=[]
 contexts=[];context_bodies=[];prefix_queries=[]
 witness_plans=[];allvars=[];queries=[];query_seeds=[];query_seed_guards=[];action_code=[];action_starts=[];query_widths=[]
 for rid,r in enumerate(rules):
  shortcut=marker_shortcuts.get((r.get('source_set'),r['name']))
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
  query_widths.append(len(slots));variants=[];prefix_variants=[];seeds=set();seed_guards={}
  rule_plans=[]
  pivots=range(len(table_atoms)) if delta_queries and r['seminaive'] and table_atoms else [None]
  for pivot in pivots:
   table_order=[];pending=list(atoms);known=set();lines=[];depth=0;stamps=[];pivot_pending=pivot is not None;seeded=False;seed_table=None;seed_atom=None;seed_row="0";seed_time="0";branching=0;split_mark=None;marker_checked=False
   if region_program is not None:
    spec=next(v for v in region_program['rules'][rid]['variants'] if v['pivot']==pivot)
    if spec['node'] is not None:
     from .regions import restore
     lines,stamps,seed_row=restore(spec,region_program['nodes'],slots,rid,pivot,parallel_match)
     known=set(spec['slots']);depth=1;table_order=list(spec['table_atoms']);branching=1
     pending=[a for i,a in enumerate(atoms) if i not in spec['prefix']]
     seeded=True;seed_atom=spec['table_atoms'][0];seed_table=ids[atoms[spec['prefix'][0]]['call']['table']]
     pivot_pending=pivot is not None and pivot not in spec['table_atoms']
     seed_time=f'(watermarks[{rid}]?{stamps[0]}:0u)' if seed_atom==pivot else '0u'
   def ready(a):return 'table' in a['call'] or all('literal' in x or x['var'] in known for x in a['args'][:-1])
   while pending:
    if shortcut and not marker_checked and all(x['operand'].get('var') in known for x in shortcut['seed']):
     marker_checked=True;tmark=ids[shortcut['table']];probe=shortcut['seed'][-1];column=probe['column'];index_columns[tmark].add(column)
     tests='&&'.join(f'old[mi*STRIDE+4+{x["column"]}]=={operand(x["operand"])}' for x in shortcut['seed'])
     lines.append('bool markerComplete=false;'+f'for(uint me=query_head(qheads,query_hash({tmark},{column},{operand(probe["operand"])}));me!=0xffffffff;me=qlinks[me]){{uint mi=me/QSTRIDE;if(me%QSTRIDE=={column}&&old[mi*STRIDE]=={tmark}&&old[mi*STRIDE+1]==0&&({tests})){{markerComplete=true;break;}}}}if(markerComplete)continue;')
    if split_matches and rid not in serial_use and branching==2:
     split_mark=(len(lines),depth,sorted(slots[n] for n in known),stamps[:],seed_time,seed_row)
    candidates=[i for i,a in enumerate(pending) if ready(a)]
    # Consume grounded filters first, then connected table atoms. This avoids
    # enumerating unrelated type/shape tables before their binding is available.
    def rank(i):
     a=pending[i]
     return (3 if 'primitive' in a['call'] else (2 if pivot_pending and a.get('_table_atom')==pivot else 0),int(a['call'].get('table') in marker_names),sum('literal' in x or x.get('var') in known for x in a['args']),-i)
    at=max(candidates,key=rank) if candidates else None
    if at is None:raise ValueError('ungrounded primitive')
    a=pending.pop(at);c=a['call']
    if 'table' in c:
     atom_index=a['_table_atom'];table_order.append(atom_index);pivot_pending=pivot_pending and atom_index!=pivot
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
      branching+=1
      column=bound[-1] if bound else cols
      if seeded or rid in serial_use:
       index_columns[t].add(column)
       if pivot is not None and atom_index==pivot:index_columns[t].add(cols+1)
      head=f'query_head(qheads,query_hash({t},{column},{operand(a["args"][column])}))' if bound else f'query_head(qheads,HASH_CAP+{t})'
      if pivot is not None and atom_index==pivot:
       timestamp_var=f'delta_ts{depth}';lines.append(f'uint {timestamp_var}=watermarks[{rid}]+1;')
       begin=f'watermarks[{rid}]?delta_first({t},m.epoch,{timestamp_var},old,qheads,qlinks):{head}'
       following=f'watermarks[{rid}]?delta_next({t},m.epoch,{entry},{timestamp_var},old,qheads,qlinks):qlinks[{entry}]'
       selected=f'(watermarks[{rid}]?TIME_COLUMN:{column})'
      else:begin=head;following=f'qlinks[{entry}]';selected=str(column)
      if not seeded:
       seed_table=t;seed_atom=atom_index
       begin=f'(QUERY_PARALLEL[set]?(lane<m.count?lane*QSTRIDE+{selected}:0xffffffff):({begin}))'
       following=f'(QUERY_PARALLEL[set]?0xffffffff:({following}))'
       seed_row=iv;seed_time=f'(watermarks[{rid}]?uint(old[{iv}*STRIDE+2]):0u)' if pivot is not None and atom_index==pivot else '0u';seeded=True
      lines.append(f'for(uint {entry}={begin};{entry}!=0xffffffff;{entry}={following}){{')
      lines.append(f'if({entry}%QSTRIDE!={selected})continue;uint {iv}={entry}/QSTRIDE;')
     lines.append(f'if(old[{iv}*STRIDE]!={t} || old[{iv}*STRIDE+1]!=0)continue;');stamps.append(f'uint(old[{iv}*STRIDE+2])')
     if rid in completed_rules:
      lines.append(f'if(bc_completed((device long*)context_info,{rid},uint(old[{iv}*STRIDE+3]),m.unions,ulong(r[RC_VISIBILITY])))continue;')
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
   suffix_order='matches[q+5+VARS]=0;matches[q+6+VARS]=sequence++;' if split_matches else 'matches[q+5+VARS]=sequence++;'
   lines.append(f'if({condition})'+'{ uint item; if(QUERY_PARALLEL[set])item=atomic_fetch_add_explicit((device atomic_uint*)&m.matched,1u,memory_order_relaxed);else item=m.matched++;if(item>=MATCHCAP){atomic_store_explicit((device atomic_uint*)&m.error,2u,memory_order_relaxed);return;}uint q=item*MSTRIDE;'+f'matches[q]={rid};'+f'for(uint c=0;c<VARS;c++)matches[q+1+c]=c<{query_widths[rid]}?v[c]:0L;matches[q+1+VARS]=rank;matches[q+2+VARS]={variant};matches[q+3+VARS]={seed_time};matches[q+4+VARS]={seed_row};'+suffix_order+'}')
   rule_plans.append({'pivot':pivot,'order':table_order,'seed':seed_atom})
   open_lines=lines[:]
   lines+=['}']*depth
   if pivot is not None:gate=f'qheads[2*QBUCKETS+1+{ids[table_atoms[pivot]["call"]["table"]]}]>watermarks[{rid}]'
   elif table_atoms and r['seminaive']:
    latest='0u'
    for atom in table_atoms:latest=f'max({latest},qheads[2*QBUCKETS+1+{ids[atom["call"]["table"]]}])'
    gate=f'{latest}>watermarks[{rid}]'
   else:gate='true'
   if not seeded:gate=f'({gate})&&(!QUERY_PARALLEL[set]||lane==0)'
   seeds.add(seed_table)
   if seed_table is not None:
    seed_gate=gate
    if pivot is not None and seed_atom<=pivot:
     relation='>' if seed_atom==pivot else '<='
     seed_gate=f'({seed_gate})&&(uint(old[lane*STRIDE+2]){relation}watermarks[{rid}])'
    seed_guards.setdefault(seed_table,[]).append(seed_gate)
   variants.append(f'if({gate})'+f'{{uint sequence=0;long v[{max(1,query_widths[rid])}]={{0}};do{{'+ '\n'.join(lines)+'}while(false);}')
   if split_mark is not None and branching>2:
    pref,body,spec=split_query(open_lines,depth,split_mark,rid,variant,gate,query_widths[rid],len(contexts))
    slot_sorts={slots[x['var']]:x['sort'] for a in r['body'] for x in a['args'] if 'var' in x}
    spec['features']=feature_layout(spec,slot_sorts)
    contexts.append(spec);context_bodies.append(body);prefix_variants.append(pref)
   else:prefix_variants.append(variants[-1])
  witness_plans.append(rule_plans)
  prefix_queries.append("\n".join(prefix_variants))
  queries.append("\n".join(variants));query_seeds.append(seeds);query_seed_guards.append(seed_guards)
  action_starts.append(len(action_code))
  def instruction(op,dst=0,sub=0,k=0,value=0):action_code.extend([op,dst,sub,k,value])
  def input_value(x):
   if 'var' in x:
    if x['var'] not in known:raise ValueError('ungrounded RHS variable')
    return 1,var(x['var'],x['sort']['name'])
   return 0,(string_ids[x['literal']] if x['sort']['kind']=='String' else x['literal'])
  def load(i,x):
   k,value=input_value(x);instruction(0,i,0,k,value)
  def identity(x):return ('v',x['var'],x['sort']['name']) if 'var' in x else ('l',x['literal'],x['sort']['name'])
  aliases={}
  def current_identity(x):return aliases.get(x['var'],identity(x)) if 'var' in x else identity(x)
  evidence={}
  for a in r['body']:
   if 'table' in a['call'] and kind[ids[a['call']['table']]]==0:
    evidence[(a['call']['table'],tuple(map(identity,a['args'][:-1])))]=a['args'][-1]
  def snapshot_input(x):
   if 'var' in x:
    index=slots[x['var']]
    if index>=query_widths[rid]:raise ValueError('context proof refers outside LHS')
    return 2,index
   return input_value(x)
  for a in r['head']:
   op=a['op']
   if op=='copy':
    k,value=input_value(a['arg']);instruction(1,var(a['dst'],a['sort']['name']),0,k,value);known.add(a['dst']);aliases[a['dst']]=current_identity(a['arg'])
   elif op=='panic':instruction(9)
   elif op=='union':
    x,y=a['args']
    if x['sort']['kind']!='eq' or x['sort']!=y['sort']:raise ValueError('union requires one equality sort')
    load(0,x);load(1,y);instruction(2)
   elif op in ('let','set','subsume'):
    c=a['call']
    proof=evidence.get((c.get('table'),tuple(map(current_identity,a['args'])))) if lhs_context_reuse and defer_unions else None
    if proof is not None and op in ('let','set'):
     k,value=snapshot_input(proof)
     if op=='let':
      instruction(1,var(a['dst'],a['sort']['name']),0,k,value);known.add(a['dst']);aliases[a['dst']]=identity(proof)
     else:
      instruction(0,0,0,k,value);load(1,a['value']);instruction(2)
     context_reuses.append({'rule':rid,'table':c['table'],'op':op})
     continue
    if op=='let':aliases[a['dst']]=('rhs',a['dst'])
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
 if region_program is not None:
  from .regions import lower as lower_regions
  region_source=lower_regions(region_program,ts,string_ids,index_columns,cols)
 joint_code,joint_info=lower_joint_polyspine(p,sets,allvars,witness_plans,joint_polyspine,polyspine_model)
 joint_enabled=joint_info.get('program') is not None
 from .stage_cell import contract as stage_contract
 baked_stages=stage_contract(p,joint_info.get('program')) if stage_contexts else None
 nv=max(query_widths+[1]);match_stride=nv+(7 if split_matches else 6)
 context_stride=max([c["words"] for c in contexts]+[CONTEXT_HEADER])
 if max(capacity*(cols+4),match_capacity*match_stride,match_capacity*context_stride,idcap)>=(1<<32):raise ValueError('device address space exhausted')
 prefix=f'#define COLS {cols}\n#define STRIDE {cols+4}\n#define VARS {nv}\n#define MSTRIDE {match_stride}\n#define CAPACITY {capacity}\n#define MATCHCAP {match_capacity}\n#define IDCAP {idcap}\n'
 hash_cap=1
 while hash_cap<capacity*4:hash_cap*=2
 radix_bits=(hash_cap+len(ts)-1).bit_length()
 radix_passes=(radix_bits+radix_digit_bits-1)//radix_digit_bits
 query_head_words=max(2*(hash_cap+len(ts))+1+len(ts),((capacity*(cols+2)+255)//256+1)*16)
 if region_program is not None:query_head_words=max(query_head_words,3*(hash_cap+len(ts))+3+len(ts))
 prefix+=f'#define CONTEXT_CAP {match_capacity}\n#define CSTRIDE {context_stride}\n'
 prefix+=f'#define NOOP_PREFLIGHT {int(noop_preflight)}\n#define REGISTERS {max([len(x) for x in allvars]+[1])}\n'
 prefix+=f'#define QUERY_PATCH {int(query_patch)}\n#define QUERY_LOG (INDEX_OFFSET+HASH_CAP+3)\n#define QUERY_OLD_VALUE (QUERY_LOG+2)\n#define QUERY_OLD_TIME (QUERY_OLD_VALUE+CAPACITY)\n#define QUERY_CHANGED (QUERY_OLD_TIME+CAPACITY)\n#define CONSTRUCTOR_PRED (QUERY_CHANGED+CAPACITY)\n#define INDEX_MERGE_VERSION (CONSTRUCTOR_PRED+CAPACITY)\n'
 prefix+=f'#define PARALLEL_MERGE_INDEX {int(parallel_merge_index)}\n#define MERGE_TARGET_INDEX {int(merge_target_index)}\n'
 prefix+=f'#define INDEXED_SUBSUME {int(indexed_subsume)}\n'
 prefix+=f'#define PARALLEL_PREPARE {int(parallel_prepare)}\n#define QUERY_RADIX_BITS {radix_bits}\n'
 prefix+=f'#define QUERY_RADIX_DIGIT_BITS {radix_digit_bits}\n#define QUERY_RADIX_PASSES {radix_passes}\n'
 prefix+=f'#define PARALLEL_REBUILD {int(parallel_rebuild)}\n'
 prefix+=f'#define UNION_CAP {union_cap}\n#define MERGE_GENERATION (IDCAP+2+2*UNION_CAP)\n#define MERGE_KEY_FLAGS (MERGE_GENERATION+1)\n'
 prefix+=f'#define INCREMENTAL_QUERY {int(incremental_queries)}\n#define QUERY_DIRTY (INDEX_OFFSET+HASH_CAP+2)\n#define QBUCKETS (HASH_CAP+TABLE_COUNT)\n'
 prefix+=f'#define QSTRIDE {cols+2}\n#define TIME_COLUMN {cols+1}\n#define TABLE_COUNT {len(ts)}\n'
 prefix+=f'#define HASH_CAP {hash_cap}\n#define INDEX_OFFSET {capacity*(cols+4)+arena_words}\n'
 prefix+=f'#define ROW_WORDS {capacity*(cols+4)}\n#define SET_WORDS {arena_words}\n'
 prefix+=f'#define RC_ENABLED {int(region_program is not None)}\n#define RC_LAZY_COMPACTION {int(lazy_contexts)}\n#define RC_VERSION (INDEX_MERGE_VERSION+1)\n#define RC_FACT_ROW (RC_VERSION+TABLE_COUNT)\n#define RC_VISIBILITY (RC_FACT_ROW+IDCAP)\n#define RC_INDEX_TOUCH (RC_VISIBILITY+1)\n#define Q_TOUCH (2*QBUCKETS+1+TABLE_COUNT)\n'
 prefix+=f'#define SC_ENABLED {int(stage_contexts)}\n#define JP_BLOCK_EFFECTS {int(block_merge and joint_enabled)}\n#define JP_BLOCK_AUDIT {int(block_merge and block_merge_audit)}\n#define JP_SET_LOG (JP_DIRTY+{(capacity*(cols+4)+127)//128 if stage_contexts else 0})\n'
 prefix+=f'#define JP_ENABLED {int(joint_enabled)}\n#define JP_STATE (INDEX_MERGE_VERSION+1)\n#define JP_NUM (JP_STATE+64)\n#define JP_HASH (JP_STATE+128)\n#define JP_VERSION (JP_HASH+HASH_CAP)\n#define JP_ROW_TAG (JP_VERSION+IDCAP)\n#define JP_DIRTY (JP_ROW_TAG+CAPACITY)\n'
 jp=joint_info.get('program');mutable=jp['mutable_tables'] if jp else [0xffffffff]*4
 prefix+='constant uint JP_IDS[]={'+','.join(map(str,mutable))+'};\n'

 set_start=[0];set_rules=[]
 for use in sets:set_rules.extend(use);set_start.append(len(set_rules))
 def merge_has_table(e):
  return isinstance(e,dict) and ('table' in e.get('call',{}) or any(merge_has_table(x) for x in e.get('args',[])))
 merge_reads=[int(merge_has_table(t.get('merge_expr'))) for t in ts]
 merge_targets=set()
 def collect_merge_targets(e):
  if not isinstance(e,dict):return
  if 'table' in e.get('call',{}):merge_targets.add(ids[e['call']['table']])
  for a in e.get('args',[]):collect_merge_targets(a)
 for t in ts:collect_merge_targets(t.get('merge_expr'))
 merge_keys=[s for i in merge_targets for s in ts[i]['schema'][:-1]]
 all_key_sorts=any(s['kind']=='set' for s in merge_keys)
 key_sorts={s['name'] for s in merge_keys if s['kind']=='eq'}
 key_sort=lambda s: int(s['kind']=='eq' and (all_key_sorts or s['name'] in key_sorts))
 prefix+=f'#define ALL_MERGE_KEY_SORTS {int(all_key_sorts)}\n'
 key_columns=[key_sort(t['schema'][c]) if c<len(t['schema']) else 0 for t in ts for c in range(cols)]
 for name,data in [('MERGE_KEY_COLUMN',key_columns),('MERGE_KEY_OUTPUT',[key_sort(t['schema'][-1]) for t in ts])]:
  prefix+='constant uint '+name+'[]={'+','.join(map(str,data))+'};\n'
 prefix+='constant uint MERGE_TARGET[]={'+','.join(str(int(i in merge_targets)) for i in range(len(ts)))+'};\n'
 if all_index_columns:index_columns=[set(range(width[i]))|{cols,cols+1} for i in range(len(ts))]
 masks=[int(c in index_columns[t]) for t in range(len(ts)) for c in range(cols+2)]
 counts=[len(s) for s in index_columns]
 for name,data in [('INDEX_COLUMN',masks),('INDEX_COUNT',counts),('WIDTH',width),('EQ',eq),('KIND',kind),('POLICY',policy),('MERGE_INDEX',merge_reads),('ACTION_START',action_starts),('QUERY_WIDTH',query_widths),('DEFER_UNIONS',deferred),('QUERY_PARALLEL',parallel_sets),('SET_START',set_start),('SET_RULES',set_rules)]:prefix+='constant uint '+name+'[]={'+','.join(map(str,data or [0]))+'};\n'
 noop_rules=[]
 for rid,(lo,hi) in enumerate(zip(action_starts,action_starts[1:])):
  # A seminaive fact-free initialization rule runs once. Do not unroll its
  # potentially huge initializer merely to test whether that one shot is empty.
  safe=bool(rules[rid]['body']) or not rules[rid]['seminaive']
  for pc in range(lo,hi,5):
   op,_,sub,_,_=action_code[pc:pc+5]
   if op in (8,9) or (op==3 and kind[sub]!=0) or (op==4 and policy[sub] not in (0,1,2,3)):safe=False
  noop_rules.append(int(safe))
 batch_code,batch_info=lower_batch_rhs(action_code,action_starts,query_widths,allvars,ts,sets,parallel_sets,deferred,batch_rhs)
 batch_hash=1
 while batch_hash<2*match_capacity:batch_hash*=2
 batch_block=64;batch_leader=batch_block+(match_capacity+255)//256;batch_prefix=batch_leader+match_capacity;batch_heads=batch_prefix+match_capacity
 prefix+=f'#define BATCH_STRIDE {cols+2}\n#define BATCH_BLOCK {batch_block}\n#define BATCH_LEADER {batch_leader}\n#define BATCH_PREFIX {batch_prefix}\n#define BATCH_HASH {batch_heads}\n'
 prefix+='constant uint BATCH_SEG[]={'+','.join(str(x) for spec in batch_info['segments'] for x in spec)+'};\n' if batch_info['segments'] else 'constant uint BATCH_SEG[]={0,0,0};\n'
 prefix+='constant uint BATCH_RULE[]={'+','.join(map(str,batch_info['eligible_mask']))+'};\nconstant uint BATCH_RANK_END[]={'+','.join(map(str,batch_info['rank_ends'] or [0]))+'};\n'
 prefix+=f'#define BATCH_ACTIVE {batch_heads+batch_hash}\n#define BATCH_EFFECT_OFFSET {batch_heads+batch_hash+match_capacity}\n'
 prefix+='constant uint BATCH_WRITES[]={'+','.join(map(str,batch_info['write_counts'] or [0]))+'};\n'
 batch_info.update(enabled=batch_rhs,minimum_parallel_bindings=256,state_words=batch_heads+batch_hash+2*match_capacity,proposal_words=match_capacity*(cols+2))
 prefix+=f'#define EFFECT_DEDUP {int(effect_dedup)}\n'
 effect_start=[0];effect_slots=[]
 for rid in range(len(rules)):
  # Every referenced initial slot is retained, even if it is later overwritten.
  # This is conservative: extra slots reduce sharing, never merge different RHS.
  slots=sorted({action_code[pc+4] for pc in range(action_starts[rid],action_starts[rid+1],5) if action_code[pc+3] and action_code[pc+4]<query_widths[rid]})
  effect_slots.extend(slots);effect_start.append(len(effect_slots))
 for name,data in [('EFFECT_SLOT_START',effect_start),('EFFECT_SLOTS',effect_slots)]:prefix+='constant uint '+name+'[]={'+','.join(map(str,data or [0]))+'};\n'
 prefix+='constant uint NOOP_RULE[]={'+','.join(map(str,noop_rules or [0]))+'};\n'
 # These proofs read only the frozen Run state. Generate scalar SSA-friendly
 # code rather than running a divergent interpreter in every GPU lane.
 def action_lines(rid,proof,union_only=False):
  lines=[f'long v{i}='+ (f'matches[mi*MSTRIDE+1+{i}]' if i<query_widths[rid] else '0L')+';' for i in range(len(allvars[rid]))]
  lines += [f'long arg{i}=0;' for i in range(cols)]
  for pc in range(action_starts[rid],action_starts[rid+1],5):
   op,dst,sub,k,literal=action_code[pc:pc+5]
   value=f'matches[mi*MSTRIDE+1+{literal}]' if k==2 else f'v{literal}' if k else ('(-9223372036854775807L-1L)' if literal==-(1<<63) else str(literal)+'L')
   if op==0:lines.append(f'arg{dst}={value};')
   elif op==1:lines.append(f'v{dst}={value};')
   elif op==2:lines.append('if(arg0!=arg1)return false;' if proof else 'parallel_unite(p,arg0,arg1,m);' if union_only else 'unite(p,arg0,arg1,m);')
   elif op in (3,4,5):
    args=','.join(f'arg{i}' for i in range(width[sub]-1)) or '0'
    lines.append(f'long key{pc}[COLS]={{'+args+'};')
    if proof:
     lines.append(f'long row{pc}=query_lookup(r,{sub},key{pc});if(row{pc}<0)return false;')
     field=f'r[row{pc}*STRIDE+{4+width[sub]-1}]'
     if op==3:lines.append(f'v{dst}={field};')
     elif op==5:lines.append(f'if(r[row{pc}*STRIDE+1]!=1)return false;')
     else:
      comparison='<' if policy[sub]==1 else '>' if policy[sub]==2 else '!='
      lines.append(f'if(arg{width[sub]-1}{comparison}{field})return false;')
    elif op==3:lines.append(f'v{dst}=write_row(r,p,fp,m,{sub},key{pc},0,true);')
    elif op==4:lines.append(f'write_row(r,p,fp,m,{sub},key{pc},arg{width[sub]-1},false);')
    else:lines.append(f'if(!hide_row(r,p,fp,m,{sub},key{pc}))return;')
   elif op==6:
    failure='return false;' if proof else 'm.error=5;return;'
    lines.append(f'if(!integer_valid({sub},arg0,arg1))'+'{'+failure+'}'+f'v{dst}=integer_eval({sub},arg0,arg1);')
   elif op==7:
    expr={0:'!arg0',1:'arg0&&arg1',2:'arg0!=arg1',3:'!arg0||arg1'}.get(sub)
    if sub==4:
     if k:raise ValueError('dynamic boolean-OR arity')
     expr='|'.join(f'arg{i}' for i in range(literal)) or '0L'
    lines.append(f'v{dst}=({expr});')
   elif op==8 and not proof:lines.append(f'v{dst}=set_apply(r,p,m,{sub},uint({value}),arg0,arg1);')
   elif op==9 and not proof:lines.append('m.error=8;return;')
   else:raise ValueError('unclassified action opcode')
   if not proof and not union_only and op in (2,3,4,5,8):lines.append('if(m.error)return;')
  if proof:lines.append('return true;')
  return ''.join(lines)
 empty_functions=[];empty_cases=[];action_functions=[];action_cases=[];compiled_actions=[]
 for rid,rule in enumerate(rules):
  if noop_rules[rid]:
   empty_functions.append(f'__attribute__((noinline)) bool empty_{rid}(uint mi,device long*matches,device long*r)'+'{'+action_lines(rid,True)+'}')
   empty_cases.append(f'case {rid}:return empty_{rid}(mi,matches,r);')
  # Keep one-shot fact initializers compact instead of unrolling thousands of
  # constructor calls. All recurring typed rule bodies get scalar direct code.
  if direct_actions and (rule['body'] or not rule['seminaive']):
   compiled_actions.append(rid)
   action_functions.append(f'__attribute__((noinline)) void action_{rid}(uint mi,device long*matches,device long*r,device uint*p,device uint*fp,device Meta&m)'+'{'+action_lines(rid,False)+'}')
   action_cases.append(f'case {rid}:action_{rid}(mi,matches,r,p,fp,m);return;')
 empty_proofs='\n'.join(empty_functions)+'\nbool actions_stably_empty(uint rule,uint mi,device long*code,device long*matches,device long*r){switch(rule){'+''.join(empty_cases)+'default:return false;}}\n'
 action_functions='\n'.join(action_functions)+'\nvoid execute_actions(uint rule,uint mi,device long*code,device long*v,device long*matches,device long*r,device uint*p,device uint*fp,device Meta&m){switch(rule){'+''.join(action_cases)+'default:execute_bytecode(rule,mi,code,v,matches,r,p,fp,m);return;}}\n'
 # This first parallel action class is deliberately exact and allocation-free.
 # The entire ruleset must commute: no masked/fallible action, read, set,
 # constructor prediction, subsume or custom callback can observe a new union.
 union_counts=[]
 for lo,hi in zip(action_starts,action_starts[1:]):
  ops=[action_code[pc] for pc in range(lo,hi,5)]
  union_counts.append(ops.count(2) if set(ops)<={0,1,2,7} else -1)
 parallel_rhs_sets=[int(parallel_rhs and parallel_rebuild and deferred[si] and parallel_sets[si] and bool(use) and all(0<=union_counts[r]<=2 for r in use) and any(union_counts[r] for r in use)) for si,use in enumerate(sets)]
 union_rules=sorted({r for si,use in enumerate(sets) if parallel_rhs_sets[si] for r in use})
 union_actions='\n'.join(f'void union_action_{rid}(uint mi,device long*matches,device uint*p,device Meta&m)'+'{'+action_lines(rid,False,True)+'}' for rid in union_rules)
 union_actions+='\nvoid union_actions(uint rule,uint mi,device long*matches,device uint*p,device Meta&m){switch(rule){'+''.join(f'case {r}:union_action_{r}(mi,matches,p,m);return;' for r in union_rules)+'default:return;}}\n'
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
 matrix_code,matrix_info=lower_matrix_sat(p,sets,ids,string_ids,matrix_saturate)
 comparison_code,comparison_info=lower_matrix_comparison(p,(matrix_saturate or joint_enabled) and parallel_rebuild)
 from .cell_program import lower as lower_cell_program
 cell_code,cell_info=lower_cell_program(cell_program,p,string_ids,joint_info)
 from .cell_updates import lower as lower_cell_updates
 update_code,update_info=lower_cell_updates(cell_program,p,string_ids)
 cell_info["update_program"]=update_info
 from .cell_spines import lower as lower_cell_spines
 spine_code,spine_info=lower_cell_spines(cell_program,p,string_ids,joint_info)
 cell_info["spine_program"]=spine_info
 ridge_code,ridge_info=lower_ridge(contexts,context_model)
 ridge_code+=f'#define CONTEXT_TRAINING {int(context_training)}\n'
 # Metal scalar types are available after the include, before generated schema.
 prefix+=f'#define UP_ENABLED {int(update_info is not None)}\n'
 prefix+=f'#define SD_ENABLED {int(spine_info["enabled"])}\n#define SD_ALL {int(spine_info["all_recipes"])}\n#define SD_WIDTH {spine_info["width"]}\n'
 prefix+=f'#define CP_ENABLED {int(cell_info["enabled"])}\n#define CP_WIDTH {cell_info["width"]}\n'
 source=(HERE/'kernel.metal').read_text().replace('// CONTEXT_RIDGE',ridge_code).replace('// MATRIX_SAT_KERNELS',(HERE/'matrix_sat.metal').read_text().replace('// MATRIX_SAT_PROGRAM',matrix_code)).replace('// JOINT_KERNELS',(HERE/'joint_polyspine.metal').read_text().replace('// JOINT_PROGRAM',joint_code).replace('// JOINT_REFRESH',(HERE/'joint_refresh.metal').read_text()).replace('// STAGE_KERNELS',(HERE/'stage_contexts.metal').read_text() if stage_contexts else '')+(HERE/'cell_program.metal').read_text().replace('// CELL_INSTRUCTIONS',cell_code)+(HERE/'cell_updates.metal').read_text().replace('// UPDATE_INSTRUCTIONS',update_code)+(HERE/'cell_spines.metal').read_text().replace('// SPINE_INSTRUCTIONS',spine_code) if joint_enabled else 'int jp_numeric_slot(uint rule,uint slot){return -1;}').replace('// COMPARISON_KERNELS',(HERE/'matrix_comparison.metal').read_text().replace('// COMPARISON_PROGRAM',comparison_code)).replace('// BATCH_KERNELS',(HERE/'batch_rhs.metal').read_text().replace('// BATCH_RECIPES',batch_code)).replace('// CONTEXT_KERNELS',(HERE/'contexts.metal').read_text()).replace('// MATCH_SORT',(HERE/'match_sort.metal').read_text()).replace('// PARALLEL_QUERY',(HERE/'query_parallel.metal').read_text()).replace('// PARALLEL_REBUILD',(HERE/'rebuild_parallel.metal').read_text()).replace('// QUERY_INDEX',(HERE/'query_index.metal').read_text()).replace('// ROW_INDEX',(HERE/'row_index.metal').read_text()).replace('// DEVICE_ACTIONS',(HERE/'actions.metal').read_text()).replace('// EMPTY_EFFECT_PROOFS',empty_proofs).replace('// DIRECT_ACTIONS',action_functions).replace('// SET_BODY',(HERE/'sets.metal').read_text()).replace('// MERGE_BODY',''.join(merge_cases)).replace('// Generated schema/limits are inserted before this code.',prefix)
 kernel_start=source.index('// Correctness-first reference executor.')
 common,template=source[:kernel_start],source[kernel_start:]
 signature=common.split('#define KERNEL_ARGS ',1)[1].splitlines()[0]
 # Helpers also run inside a device-controlled local schedule, whose ruleset
 # ID is a thread value rather than a host-bound constant-buffer reference.
 signature=re.sub(r'\[\[.*?\]\]','',signature).replace('constant uint&set','uint set')+',uint rank'
 call_args='r,old,p,fp,m,matches,watermarks,set,code,registers,qheads,qlinks,control,match_order,noop,qprev,contexts,context_info,context_results,lane'
 query_functions='\n'.join(f'__attribute__((noinline)) void query_{rid}({signature})'+'{'+q+'}' for rid,q in enumerate(queries))
 context_rules={c['rule'] for c in contexts}
 prefix_functions='\n'.join(f'__attribute__((noinline)) void prefix_{rid}({signature})'+'{'+prefix_queries[rid]+'}' for rid in sorted(context_rules))
 # Context suffixes restore rank from their own immutable header.
 suffix_signature=signature.rsplit(',uint rank',1)[0]
 suffix_functions='\n'.join(f'__attribute__((noinline)) void suffix_{i}({suffix_signature})'+'{'+body+'}' for i,body in enumerate(context_bodies))
 suffix_cases=''.join(f'case {i}:suffix_{i}({call_args});break;' for i in range(len(contexts)))
 context_kernel='kernel void context_resume(KERNEL_ARGS,constant uint&phase[[buffer(24)]]){if(m.error||context_info[1]||lane>=context_info[0])return;if(CONTEXT_RIDGE&&uint(context_results[lane*2]>=0.5f)!=phase)return;switch(uint(contexts[lane*CSTRIDE])){'+suffix_cases+'default:atomic_store_explicit((device atomic_uint*)&m.error,15u,memory_order_relaxed);}}'
 kernels=[];kernel_names=[];prefix_names=[];fallback_names=[]
 for si,use in enumerate(sets):
  name=f'step_{si}';kernel_names.append(name)
  def call(r,rank):return f'query_{r}({call_args},{rank});if(atomic_load_explicit((device atomic_uint*)&m.error,memory_order_relaxed))return;'
  if seed_dispatch and parallel_sets[si]:
   # A physical row can seed only variants whose first branching table has
   # this type. Grounded variants still run exactly once, including count=0.
   grounded=''.join(call(r,rank) for rank,r in enumerate(use) if None in query_seeds[r])
   cases=[]
   for table in sorted({t for r in use for t in query_seeds[r] if t is not None}):
    body=''
    for rank,r in enumerate(use):
     if table in query_seeds[r]:
      q=call(r,rank);conditions=[]
      if None in query_seeds[r]:conditions.append('lane!=0')
      if early_seed_guards:
       gates=list(dict.fromkeys(query_seed_guards[r][table]))
       if 'true' not in gates:conditions.append('('+'||'.join('('+g+')' for g in gates)+')')
      body+=('if('+'&&'.join(conditions)+'){'+q+'}') if conditions else q
    cases.append(f'case {table}:'+'{'+body+'break;}')
   calls='if(lane==0){'+grounded+'}if(lane>=m.count||old[lane*STRIDE+1]!=0)return;switch(uint(old[lane*STRIDE])){'+''.join(cases)+'default:break;}'
  else:calls=''.join(call(r,rank) for rank,r in enumerate(use))
  kernels.append(template.replace('kernel void step(',f'kernel void {name}(').replace('// MATCH_BODY',calls))
  use_contexts=any(c['rule'] in use for c in contexts)
  prefix_names.append('prefix_'+name if use_contexts else None);fallback_names.append('fallback_'+name if use_contexts else None)
  if use_contexts:
   prefix_calls=calls
   for r in context_rules.intersection(use):prefix_calls=prefix_calls.replace(f'query_{r}(',f'prefix_{r}(')
   kernels.append(template.replace('kernel void step(',f'kernel void prefix_{name}(').replace('// MATCH_BODY',prefix_calls))
   kernels.append(template.replace('kernel void step(',f'kernel void fallback_{name}(').replace('// MATCH_BODY','if(!context_info[1])return;'+calls))
 if scheduled_info is not None:common+=(HERE/'scheduled_matrix.metal').read_text()
 if batch_enabled:region_source+='\nbool bc_completed(device long*c,uint rule,uint fid,uint unions,ulong visibility);\n'
 source=common+region_source+query_functions+prefix_functions+suffix_functions+context_kernel+'\n'.join(kernels)
 source=source.replace('// PARALLEL_UNION_ACTIONS',union_actions)
 return {'region_program':region_program,'marker_program':None if marker_program is None else {'sha256':marker_program['sha256'],'source_sha256':marker_program['source_sha256'],'tables':sorted(marker_names),'consumer_reads':sum(len(m['consumers']) for m in marker_program['markers']),'publication_sites':sum(len(m['publishers']) for m in marker_program['markers']),'functional_completion_shortcuts':len(marker_shortcuts),'execution':'source-defined Unit views; original stage completion; marker-preferred exact queries'},'scheduled_matrix':scheduled_info,'schema':'egglog-device-storage/v1','cell_program':cell_info,'block_merge':bool(block_merge and joint_enabled),'block_merge_audit':bool(block_merge and block_merge_audit),'joint_polyspine':joint_info,'stage_cell':baked_stages,'polyspine_training':polyspine_training,'polyspine_full_refresh':polyspine_full_refresh,'polyspine_refresh_audit':polyspine_refresh_audit,'batch_rhs':batch_info,'matrix_saturate':matrix_info,'comparison_saturate':comparison_info,'rebuild_batch':rebuild_batch,'merge_target_index':merge_target_index,'merge_target_tables':[ts[i]['name'] for i in sorted(merge_targets)],'context_training':context_training,'context_ridge':ridge_info,'split_matches':split_matches,'query_contexts':contexts,'context_capacity':match_capacity if contexts else 0,'context_stride':context_stride,'prefix_kernel_names':prefix_names,'fallback_kernel_names':fallback_names,'parallel_rhs_sets':parallel_rhs_sets,'parallel_rhs_rules':union_rules,'seed_dispatch':seed_dispatch,'noop_preflight':noop_preflight,'direct_actions':direct_actions,'query_patch':query_patch,'effect_dedup':effect_dedup,'early_seed_guards':early_seed_guards,'lhs_context_reuse':lhs_context_reuse,'rhs_context_reuses':context_reuses,'effect_input_slots':[effect_slots[effect_start[i]:effect_start[i+1]] for i in range(len(rules))],'query_prev_words':2*capacity,'compiled_action_rules':compiled_actions,'noop_eligible_rules':noop_rules,'query_seed_tables':[[ts[t]['name'] if t is not None else None for t in sorted(seeds,key=lambda x:-1 if x is None else x)] for seeds in query_seeds],'parallel_rebuild':parallel_rebuild,'parallel_match_sets':parallel_sets,'indexed_subsume':indexed_subsume,'parallel_merge_index':parallel_merge_index,'index_columns':{t['name']:sorted(index_columns[i]) for i,t in enumerate(ts)},'match_order_capacity':max(256,1<<(match_capacity-1).bit_length()),'parallel_prepare':parallel_prepare,'query_radix_bits':radix_bits,'query_radix_passes':radix_passes,'query_radix_digit_bits':radix_digit_bits,'query_sort_words':capacity*(cols+2),'hash_capacity':hash_cap,'shader':source,'kernel_names':kernel_names,'action_code':action_code,'register_count':max([len(x) for x in allvars]+[1]),'set_rule_ids':sets,'query_dirty_offset':capacity*(cols+4)+arena_words+hash_cap+2,'tables':ts,'strings':strings,'initial_rows':flat,'initial_sets':arena,'set_words':arena_words,'index_words':hash_cap+6+4*capacity+(len(ts)+idcap+hash_cap+2 if region_program is not None else 0)+(128+hash_cap+idcap+capacity+((capacity*(cols+4)+127)//128 if stage_contexts else 0)+(3*match_capacity if block_merge else 0) if joint_enabled else 0),'incremental_queries':incremental_queries,'delta_queries':delta_queries,'index_counter_offset':capacity*(cols+4)+arena_words+hash_cap+1,'union_words':3+union_cap*2+idcap,'deferred_rulesets':[s['name'] for i,s in enumerate(p['rulesets']) if deferred[i]],'query_head_words':query_head_words,'query_link_words':capacity*(cols+2),'anchors':anchors,'sets':[s['name'] for s in p['rulesets']],'schedule':p['schedule'],'assertion_tables':p.get('assertion_tables',[]),'capacity':capacity,'match_capacity':match_capacity,'workspace_words':max(match_capacity*match_stride,capacity*(cols+4)),'id_capacity':idcap,'stride':cols+4,'match_stride':match_stride,'rules':len(rules),'meta':[len(p['initial']),len(anchors)+1,1,0,0,0,0,0,len(p['initial'])+1]}

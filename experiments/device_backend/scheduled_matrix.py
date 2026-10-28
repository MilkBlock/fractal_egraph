"""Compile real schedule subtrees into private, stage-gated matrix completion.

The native typed rules establish the fixed-carrier proof; observation is not a
proof of absent matches. Unsupported subtrees remain in the existing executor.
"""
import copy,hashlib,json
from matrix_sat import analyze,escaping_results

def key(x):return json.dumps(x,sort_keys=True,separators=(',',':'))
def leaves(s):
 if 'run' in s:return [s['run']]
 if 'sequence' in s:return [n for b in s['sequence'] for n in leaves(b)]
 return leaves(s.get('body',s.get('saturate',{})))

def control(s,ids):
 code=[];depth=0;peak=0
 def emit(s):
  nonlocal depth,peak
  if 'run' in s:
   if s.get('until') or s.get('until_run') or s.get('assert'):raise ValueError('external check is a publication boundary')
   code.append([0,ids[s['run']],0,0]);return
  if 'sequence' in s:
   for b in s['sequence']:emit(b)
   return
  repeat='repeat' in s
  n=s['repeat'] if repeat else 0
  if not isinstance(n,int) or not 0<=n<2**32:raise ValueError('invalid finite Repeat')
  at=len(code);code.append([1 if repeat else 2,n,0,0]);depth+=1;peak=max(depth,peak)
  emit(s['body'] if repeat else s['saturate']);depth-=1
  code.append([3,at,0,0]);code[at][2]=len(code)
 emit(s);code.append([4,0,0,0])
 return {'instructions':code,'stack_depth':peak,'repeat_stop':'can_stop','saturate_stop':'not updated'}

def prepare(core):
 out=copy.deepcopy(core);byname={s['name']:s for s in core['rulesets']};ids={s['name']:i for i,s in enumerate(core['rulesets'])}
 programs=[];rejected=[];seen={}
 escaped=escaping_results(core)
 tables={t['name']:t for t in core['tables']}
 def blocked_ruleset(rs):
  for r in rs['rules']:
   if any('table' not in a['call'] for a in r['body']):return True
   for a in r['head']:
    if a['op']=='copy':continue
    if a['op']!='let' or 'table' not in a.get('call',{}):return True
    t=tables[a['call']['table']]
    if t['name'] in escaped:return True
    if t['kind']!='constructor' or len(t['schema'])!=2 or any(v['kind']!='eq' for v in t['schema']):return True
  return False
 barriers={name for name,rs in byname.items() if blocked_ruleset(rs)}
 def barrier(s):
  if 'run' in s:return bool(s.get('until_run') or s.get('assert') or s['run'] in barriers)
  if 'sequence' in s:return any(barrier(b) for b in s['sequence'])
  if 'repeat' in s:return barrier(s['body'])
  if 'saturate' in s:return barrier(s['saturate'])
  return False
 def try_program(s):
  k=key(s)
  if k in seen:return seen[k]
  names=list(dict.fromkeys(leaves(s)))
  if not names:return None
  try:
   ctrl=control(s,ids)
   rules=[];known=set()
   for name in names:
    for r in byname[name]['rules']:
     rid=(r.get('source_set',name),r['name'])
     if rid not in known:known.add(rid);rules.append(r)
   if any(a.get('call',{}).get('table') in escaped for r in rules for a in r['head']):raise ValueError('result identity escapes beyond the admitted region')
   # This admission validates the complete union, including cross-stage reads.
   private_name=f'__scheduled_matrix_{len(programs)}'
   while any(r['name']==private_name for r in out['rulesets']):private_name+='_'
   combined={'name':private_name,'rules':rules,'scheduled_matrix':True}
   accepted,why=analyze({'tables':core['tables'],'rulesets':[combined]},[list(range(len(rules)))])
   if not accepted:raise ValueError(why[0]['reason'])
   sid=len(out['rulesets']);out['rulesets'].append(combined)
   item={'index':len(programs),'set':sid,'schedule':s,'control':ctrl,'rulesets':names,
         'contract':'positive fixed-carrier closure with private predicate identities',
         'rule_sha256':hashlib.sha256(key(rules).encode()).hexdigest()}
   programs.append(item);seen[k]=item;return item
  except (ValueError,KeyError) as e:
   rejected.append({'rulesets':names,'reason':str(e)});seen[k]=None;return None
 def visit(s):
  if try_program(s) is not None:return
  if 'sequence' in s:
   # Maximal neighboring regions, not arbitrary combinations of Cell paths.
   children=s['sequence'];i=0
   while i<len(children):
    end=i+1
    for stop in range(i+2,len(children)+1):
     region={'sequence':children[i:stop]}
     # A later predicate producer can repair an earlier missing-port failure.
     # Only a semantic publication barrier rules out every larger region.
     if barrier(region):break
     if try_program(region) is not None:end=stop
    if end==i+1:visit(children[i]) if 'run' not in children[i] else None
    i=end
  elif 'repeat' in s:visit(s['body'])
  elif 'saturate' in s:visit(s['saturate'])
 for s in core['schedule']:visit(s)
 return out,{'schema':'scheduled-matrix-cell/v1','programs':programs,'rejected':rejected,
             'original_schedule':core['schedule'],'publication':'end of the admitted original subtree',
             'unknown_program':'ordinary GPU execution at original stage'}

def bind_analyzer(library,core,compiled,source_text):
 """Analyzer supplies original control and observed sites; typed lowering still
 proves completeness of the admitted rules, never infers it from frequency."""
 from joint_polyspine import normalize_schedule
 if library.get('schema')!='analyzer-cell-recipes/v1':raise ValueError('unsupported analyzer recipe library')
 source=library.get('source_schedule')
 if not source or source.get('schema')!='cell-schedule/v1':raise ValueError('library has no original schedule; rebake with stage evidence')
 h=0xcbf29ce484222325
 for b in source_text.encode():h=((h^b)*0x100000001b3)&((1<<64)-1)
 if source.get('source_fingerprint')!={'bytes':len(source_text.encode()),'fnv1a64':f'{h:016x}'}:raise ValueError('analyzer source fingerprint differs; rebake the stage evidence')
 def convert(n):
  op=n['op']
  if op=='run':
   return {'run':n['ruleset'],**({'has_until':True} if n.get('until') is not None else {})}
  if op=='sequence':return {'sequence':[convert(b) for b in n['body']]}
  if op=='repeat':return {'repeat':n['count'],'body':convert(n['body'])}
  if op=='saturate':return {'saturate':convert(n['body'])}
  raise ValueError('unknown analyzer control opcode')
 expected=[normalize_schedule(convert(c['program'])) for c in source['commands']]
 def actual_node(s):
  if 'run' in s:return {'run':s['run'],**({'has_until':True} if s.get('until_run') else {})}
  if 'sequence' in s:return {'sequence':[actual_node(b) for b in s['sequence']]}
  if 'repeat' in s:return {'repeat':s['repeat'],'body':actual_node(s['body'])}
  return {'saturate':actual_node(s['saturate'])}
 actual=[normalize_schedule(actual_node(s)) for s in core['schedule'] if not s.get('assert')]
 if expected!=actual:raise ValueError('analyzer schedule differs from runtime schedule')
 events=library.get('stage_events',[])
 if not isinstance(events,list):raise ValueError('invalid native Run boundaries')
 validate_stage_evidence(source,events)
 observed={e['ruleset'] for e in events}
 # Instructions keep all native rules; stage observations provide provenance,
 # not permission to drop unobserved branches or future input bindings.
 admitted=compiled['scheduled_matrix']
 admitted['analyzer_evidence']={'schema':'cell-stage-evidence/v1','events':len(events),
  'rulesets':sorted(observed),'prepared_cells':library.get('registered_with_program',0),
  'schedule_equal':True,'matching_coverage_proof':'complete current typed rules; not observed trace frequency'}
 for p in admitted['programs']:
  p['observed_rulesets']=sorted(set(p['rulesets'])&observed)
 return admitted['analyzer_evidence']

def validate_stage_evidence(source,events):
 """Replay only control decisions, independently of GPU control bytecode."""
 at=0
 def visit(n,path,iterations):
  nonlocal at
  op=n['op']
  if op=='run':
   if at==len(events):raise ValueError('truncated native stage evidence')
   e=events[at];at+=1
   if e.get('path')!=path or e.get('iterations')!=iterations or e.get('ruleset')!=n['ruleset']:raise ValueError('native stage position differs from source schedule')
   if not isinstance(e.get('updated'),bool) or not isinstance(e.get('can_stop'),bool):raise ValueError('missing native stage progress')
   if e['updated'] and e['can_stop']:raise ValueError('inconsistent native stage progress')
   return e['updated'],e['can_stop']
  changed=False;stop=True
  if op=='sequence':
   for i,b in enumerate(n['body']):
    c,s=visit(b,path+[i],iterations);changed|=c;stop&=s
  elif op in ('repeat','saturate'):
   i=0
   while op=='saturate' or i<n['count']:
    c,s=visit(n['body'],path+[0],iterations+[i]);changed|=c;stop&=s;i+=1
    if (op=='repeat' and s) or (op=='saturate' and not c):break
  else:raise ValueError('unknown native control opcode')
  return changed,stop
 for command in source['commands']:visit(command['program'],[command['command']],[])
 if at!=len(events):raise ValueError('extra native stage evidence beyond the source schedule')
 return at

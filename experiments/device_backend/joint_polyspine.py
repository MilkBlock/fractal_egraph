"""Compile the shared analyzer marker ABI into a complete nested terms program.

The existing affine/marker compiler supplies ports and numeric DAGs. This
module additionally checks the full RHS, helper effects, merge and schedule;
the old minimum-cost projection is never used as a full closure certificate.
"""
import hashlib,json,math,struct,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:sys.path.insert(0,str(ROOT))
from experiments.polyspine_ridge.resumable_program import compile_links
from tools.cell_spine.interfaces import native_row_interface
from matrix_comparison import recognize

def normalize_schedule(s):
    if 'sequence' in s:
        xs=[normalize_schedule(x) for x in s['sequence']]
        return xs[0] if len(xs)==1 else {'sequence':xs}
    if 'saturate' in s:return {'saturate':normalize_schedule(s['saturate'])}
    if 'repeat' in s:return {'repeat':s['repeat'],'body':normalize_schedule(s['body'])}
    return s

def fingerprint(x):return hashlib.sha256(json.dumps(x,sort_keys=True,separators=(',',':')).encode()).hexdigest()

def bind_roles(atom,roles,env):
    if len(atom['args'])!=len(roles):raise ValueError('helper arity')
    for a,role in zip(atom['args'],roles):
        if 'var' not in a or (a['var'] in env and env[a['var']]!=role):raise ValueError('extra helper binding constraint')
        env[a['var']]=role

def helper_contract(rule,phase,tables):
    atoms=[a for a in rule['body'] if 'table' in a['call']];guards=[a for a in rule['body'] if 'primitive' in a['call']]
    env={};roles=[];pot=[a for a in atoms if a['call']['table']=='PotentialExtractedExpr']
    if len(pot)!=1:raise ValueError('one potential row required per helper')
    bind_roles(pot[0],['root','candidate','potential'],env)
    if phase==0:
        if len(atoms)!=1 or guards:raise ValueError('initializer body')
    else:
        funcs=[a for a in atoms if a['call']['table']=='ExtractedExpr'];costs=[a for a in atoms if a['call']['table']=='TCPair']
        if len(guards)!=1 or guards[0]['call']!={'primitive':'<','inputs':['i64','i64'],'output':'Unit'}:raise ValueError('helper guard')
        if phase==1:
            if len(funcs)!=1 or len(costs)!=2 or len(atoms)!=4:raise ValueError('update helper body')
            bind_roles(funcs[0],['root','old'],env)
        elif funcs or len(costs)!=1 or len(atoms)!=2:raise ValueError('panic helper body')
        for a in costs:
            returned=a['args'][-1].get('var');role=env.get(returned)
            if role not in ['candidate','old']:raise ValueError('unrelated cost witness')
            bind_roles(a,[role+'_term',role+'_cost',role],env)
        args=guards[0]['args'];expected=['candidate_cost','old_cost'] if phase==1 else ['candidate_cost',0]
        actual=[env.get(a.get('var')) if 'var' in a else a['literal'] for a in args[:2]]
        if actual!=expected:raise ValueError('changed helper condition')
    def value(a):return a['literal'] if 'literal' in a else env[a['var']]
    made=[];effects=[]
    for a in rule['head']:
        if a['op']=='copy':env[a['dst']]=value(a['arg'])
        elif a['op']=='let' and 'table' in a['call']:
            expr=[a['call']['table'],*[value(x) for x in a['args']]];env[a['dst']]=expr;made.append(expr)
        elif a['op']=='set':effects.append([a['call']['table'],*[value(x) for x in a['args']],value(a['value'])])
        elif a['op']=='panic':effects.append(['panic'])
        else:raise ValueError('extra helper effect')
    if phase==0:
        if len(made)!=2 or len(made[0])!=1 or made[1][:2]!=['TCPair',made[0]] or not isinstance(made[1][-1],int):raise ValueError('initializer construction')
        if effects!=[['ExtractedExpr','root',made[1]]]:raise ValueError('initializer function update')
        init=tables[made[0][0]]
        if init['kind']!='constructor' or len(init['schema'])!=1 or init['schema'][0]['name']!=tables['TCPair']['schema'][0]['name']:raise ValueError('initializer term sort')
    elif phase==1:
        if made!=[['TCPair','candidate_term','candidate_cost']] or effects!=[['ExtractedExpr','root',made[0]]]:raise ValueError('update helper effects')
    elif made or effects!=[['panic']]:raise ValueError('panic helper effects')
    # Assign each original table atom to the complete row witness retained by
    # the context; no independent marginal is ever used to build a binding.
    for a in atoms:
        name=a['call']['table']
        if name=='PotentialExtractedExpr':roles.append(0)
        elif name=='ExtractedExpr':roles.append(1)
        else:
            ret=a['args'][-1]['var'];roles.append(2 if env[ret]=='candidate' else 3)
    return roles

FEATURES=['bias','phase_terms','phase_helpers','phase_comparison','has_result','min_cost_magnitude','max_cost_magnitude','cost_alternatives',
          'new_result','generation','spine_count','producer_degree','consumer_degree','ready_ports','pending_ports','aliased_ports',
          'resolved_root','pure_root','typed_root','resolved_ports','pure_ports','typed_ports','potential_count','candidate_reads',
          'choice_count','improving_exists','nonimproving_exists','changed_previous_phase','effect_rows_previous_phase',
          'number_zero','number_positive','number_negative']

def codebook(tokens,width=64):
    # Identical SplitMix/bit code used by superposed_key_memory.memory.codes;
    # no NumPy dependency is needed by native program compilation.
    mask=(1<<64)-1;out=[]
    for token in tokens:
        key=int.from_bytes(hashlib.blake2b(json.dumps(('resident-feature',token),separators=(',',':')).encode(),digest_size=8,person=b'cell-key').digest(),'little')
        row=[]
        for word in range(width//64):
            x=key^(((word+1)*0x9e3779b97f4a7c15)&mask);x=((x^(x>>30))*0xbf58476d1ce4e5b9)&mask;x=((x^(x>>27))*0x94d049bb133111eb)&mask;x^=x>>31
            row.extend(1 if (x>>i)&1 else -1 for i in range(64))
        out.append(row)
    return out

def compile_contract(core,sets,allvars,orders):
    linked=compile_links(core);rs={r['name']:r for r in core['rulesets']};tables={t['name']:t for t in core['tables']};table_ids={t['name']:i for i,t in enumerate(core['tables'])}
    names=['terms','terms-helpers','terms-helpers-helpers'];setids=[next(i for i,r in enumerate(core['rulesets']) if r['name']==n) for n in names]
    comparison=recognize(rs[names[2]],tables)
    if comparison['cost_table']!='TCPair' or comparison['choice_table']!='Smaller':raise ValueError('comparison does not implement the shared terms ABI')
    fn=tables['ExtractedExpr'];expected={'call':{'table':'Smaller'},'args':[{'var':'old'},{'var':'new'}]}
    if fn['kind']!='function' or fn['merge_expr']!=expected:raise ValueError('changed extraction merge program')
    if len(rs[names[1]]['rules'])!=3:raise ValueError('helper ruleset must be complete')
    recipes=[];plans=[]
    for i,(r,recipe) in enumerate(zip(rs[names[0]]['rules'],linked['recipes'])):
        rid=sets[setids[0]][i];anchor=next(a for a in r['body'] if a['call'].get('table')==recipe['anchor'])
        if any('var' not in a for a in anchor['args']):raise ValueError('literal seed fields require specialization')
        if len({a['var'] for a in anchor['args']})!=len(anchor['args']):raise ValueError('seed aliases require specialization')
        cols={a['var']:j for j,a in enumerate(anchor['args'])};roles=[]
        for a in r['body']:
            name=a['call'].get('table')
            if name==recipe['anchor']:roles.append(0)
            elif name=='ExtractedExpr':roles.append(1+next(j for j,p in enumerate(recipe['inputs']) if p['key']==a['args'][0]['var']))
            elif name=='TCPair':roles.append(4+next(j for j,p in enumerate(recipe['inputs']) if p['return']==a['args'][-1]['var']))
            else:raise ValueError('additional seed condition')
        made=[];numeric=[]
        for a in r['head']:
            if a['op']=='copy':continue
            if a['op']!='let':raise ValueError('seed has nonconstructive effects')
            if 'primitive' in a['call']:numeric.append(allvars[rid][a['dst']]);continue
            t=tables[a['call']['table']]
            if t['kind']!='constructor':raise ValueError('seed function call')
            if t['schema'][-1]['name']==tables['ExtractedExpr']['schema'][0]['name']:raise ValueError('seed grows the structural carrier')
            made.append(t['name'])
        if made.count('TCPair')!=1 or made.count('PotentialExtractedExpr')!=1 or made[-1]!='PotentialExtractedExpr':raise ValueError('seed output interface')
        interface=native_row_interface(anchor,recipe['inputs'])
        recipes.append(dict(recipe,rule=rid,table=table_ids[recipe['anchor']],interface=interface,
                            root_column=len(anchor['args'])-1,numeric_slots=numeric,root_slot=allvars[rid][anchor['args'][-1]['var']]))
        plans.append({'rule':rid,'rank':i,'roles':roles,'rule_ast':r,'orders':orders[rid],'slots':allvars[rid]})
    if len({r['table'] for r in recipes})!=len(recipes):raise ValueError('ambiguous structural publisher table')
    for i,r in enumerate(rs[names[1]]['rules']):
        rid=sets[setids[1]][i];roles=helper_contract(r,i,tables)
        plans.append({'rule':rid,'rank':i,'roles':roles,'rule_ast':r,'orders':orders[rid],'slots':allvars[rid]})
    # Existing comparison schema proof protects all equality feedback. New
    # constructors must not invalidate the frozen source carrier/interface.
    mutable={'TCPair','PotentialExtractedExpr','ExtractedExpr','Smaller'}
    written={a['call']['table'] for n in names for r in rs[n]['rules'] for a in r['head'] if 'table' in a.get('call',{})}
    if written & {r['anchor'] for r in recipes}:raise ValueError('rules modify a seed anchor table')
    schedule={'saturate':{'sequence':[{'run':names[0]},{'saturate':{'sequence':[{'run':names[1]},{'saturate':{'run':names[2]}}]}}]}}
    features=FEATURES+['recipe:'+r['anchor'] for r in recipes]+[f'{name}_byte_{i}' for name in ['root_min','root_max','candidate_min','candidate_max'] for i in range(8)];width=(len(features)+7)//8*8
    features+=['padding:'+str(i) for i in range(width-len(features))]
    layout={'schema':'resident-polyspine-features/v1','features':features,'encoded_width':128,'ridge_width':136,'heads':['terms_effect','helper_effect','comparison_effect'],
            'interfaces':[r['interface'] for r in recipes],'program_rules':{n:rs[n]['rules'] for n in names},'encoder':'cell-key SplitMix signed bits'}
    encoding=codebook(features);role_encoding=codebook([f'port:{r["anchor"]}:{j}' for r in recipes for j in range(3)])
    rotations=[int(fingerprint([r['anchor'],j])[:8],16)%64 for r in recipes for j in range(3)]
    layout.update(encoding_sha256=fingerprint(encoding),role_encoding_sha256=fingerprint(role_encoding),role_rotations=rotations,
                  head_codebook_probe=codebook(['head:__sort__:__table__'])[0])
    root_sort=fn['schema'][0]['name']
    head_tables=[t['kind']=='constructor' and t['schema'][-1]['kind']=='eq' and t['schema'][-1]['name']==root_sort for t in core['tables']]
    return {'schema':'resident-polyspine-program/v1','set_ids':setids,'sets':names,'schedule':schedule,'recipes':recipes,'plans':plans,
            'mutable_tables':[table_ids[n] for n in ['TCPair','PotentialExtractedExpr','ExtractedExpr','Smaller']],
            'property_tables':[table_ids.get(n,-1) for n in ['ExprIsResolved','ExprIsPure','HasType']],
            'comparison':comparison,'feature_layout':layout,'layout_sha256':fingerprint(layout),'encoding':encoding,
            'head_tables':head_tables,'head_encoding':codebook(['head:'+root_sort+':'+t['name'] for t in core['tables']]),
            'role_encoding':role_encoding,'role_rotation':rotations,
            'feature_width':width,'rules_sha256':fingerprint(layout['program_rules']),'initializer_cost':linked['initializer_cost']}

def lower(core,sets,allvars,orders,enabled,model=None):
    if not enabled:return '',{'enabled':False,'program':None}
    try:p=compile_contract(core,sets,allvars,orders)
    except (ValueError,KeyError,StopIteration) as e:return '',{'enabled':True,'program':None,'rejected':str(e)}
    if model is not None:
        if {'encoding','recipes','affine_weights','program_rules'} & set(model):raise ValueError('a learned model cannot replace fixed encoding or effect programs')
        if model.get('schema')!='resident-polyspine-ridge/v1' or model.get('layout_sha256')!=p['layout_sha256']:raise ValueError('Polyspine model layout/program mismatch')
        ws=model.get('weights');sc=model.get('scales');bias=model.get('bias',[0,0,0])
        if not isinstance(ws,list) or len(ws)!=136*8 or len(sc or [])!=3 or any(not math.isfinite(v) for v in ws+sc) or any(abs(v)>65504 for v in ws) or any(v<=0 or v>1e6 for v in sc) or len(bias)!=3 or any(not math.isfinite(v) or abs(v)>1e6 for v in bias):raise ValueError('invalid Polyspine ridge coefficients')
        p['model']=model
    else:p['model']=None
    # Generic lowering is filled by the runtime compiler after this admission.
    return metal_program(p,core),{'enabled':True,'program':p}

def metal_program(p,core):
    out=[];rec=p['recipes'];plans=p['plans'];tables={t['name']:i for i,t in enumerate(core['tables'])}
    const=lambda name,vals:out.append('constant uint '+name+'[]={'+','.join(map(str,vals))+'};')
    out.append(f'#define JP_RECIPES {len(rec)}\n#define JP_FEATURES {p["feature_width"]}\n#define JP_OPS {max([len(r["operations"]) for r in rec]+[1])}\n#define JP_ATOMS {max(len(q["roles"]) for q in plans)}')
    for name,values in [('JP_SETS',p['set_ids']),('JP_MUTABLE',p['mutable_tables']),('JP_PROPERTIES',[x if x>=0 else 0xffffffff for x in p['property_tables']]),('JP_TERM_RULES',[r['rule'] for r in rec]),('JP_HELPER_RULES',[q['rule'] for q in plans[len(rec):]]),('JP_ARITY',[len(r['inputs']) for r in rec])]:const(name,values)
    const('JP_HEAD_TABLE',[int(x) for x in p['head_tables']]);const('JP_ROLE_ROTATION',p['role_rotation'])
    out.append('bool jp_numeric_anchor(uint recipe,uint col){switch(recipe){'+''.join(f'case {i}:return '+('||'.join(f'col=={j}' for j,a in enumerate(r['args']) if a['sort']['kind']=='i64') or 'false')+';' for i,r in enumerate(rec))+'default:return false;}}')
    out.append('int jp_recipe(uint table){switch(table){'+''.join(f'case {r["table"]}:return {i};' for i,r in enumerate(rec))+'default:return -1;}}')
    out.append('uint jp_child(uint recipe,uint child){switch(recipe){'+''.join(f'case {i}:{{const uint cols[]={{'+','.join(str(x['column']) for x in r['inputs'] or [{'column':0}])+'};return cols[child];}' for i,r in enumerate(rec))+'default:return 0;}}')
    out.append('uint jp_root_col(uint recipe){switch(recipe){'+''.join(f'case {i}:return {r["root_column"]};' for i,r in enumerate(rec))+'default:return 0;}}')
    out.append('int jp_numeric_slot(uint rule,uint slot){switch(rule){'+''.join(f'case {r["rule"]}:switch(slot){{'+''.join(f'case {s}:return {j};' for j,s in enumerate(r['numeric_slots']))+'default:return -1;}' for r in rec)+'default:return -1;}}')
    out.append('uint jp_atom_count(uint rule){switch(rule){'+''.join(f'case {q["rule"]}:return {len(q["roles"])};' for q in plans)+'default:return 0;}}')
    out.append('uint jp_rule_rank(uint rule){switch(rule){'+''.join(f'case {q["rule"]}:return {q["rank"]};' for q in plans)+'default:return 0;}}')
    out.append('uint jp_order_atom(uint rule,uint pivot,uint pos){switch(rule){'+''.join(f'case {q["rule"]}:switch(pivot){{'+''.join(f'case {o["pivot"] if o["pivot"] is not None else 0}:{{const uint ord[]={{'+','.join(map(str,o['order']))+'};return ord[pos];}' for o in q['orders'])+'default:return 0;}' for q in plans)+'default:return 0;}}')
    out.append('uint jp_seed_atom(uint rule,uint pivot){switch(rule){'+''.join(f'case {q["rule"]}:switch(pivot){{'+''.join(f'case {o["pivot"] if o["pivot"] is not None else 0}:return {o["seed"] if o["seed"] is not None else 0xffffffff};' for o in q['orders'])+'default:return 0;}' for q in plans)+'default:return 0;}}')
    out.append('int jp_result_op(uint recipe){switch(recipe){'+''.join(f'case {i}:return {r["result"][1]-3 if r["result"][0]==0 and r["result"][1]>=3 else -1};' for i,r in enumerate(rec))+'default:return -1;}}')
    out.append('bool jp_seminaive(uint rule){switch(rule){'+''.join(f'case {q["rule"]}:return {str(q["rule_ast"]["seminaive"]).lower()};' for q in plans)+'default:return true;}}')
    out.append('bool jp_delta(uint rule){switch(rule){'+''.join(f'case {q["rule"]}:return {str(q["orders"][0]["pivot"] is not None).lower()};' for q in plans)+'default:return false;}}')
    # Bind all body fields from correlated witnesses; generated helpers retain
    # primitive guards. Matrix results later replace only certified arithmetic.
    cases=[]
    for q in plans:
        lines=[];known=set();atoms=[a for a in q['rule_ast']['body'] if 'table' in a['call']]
        for j,(a,role) in enumerate(zip(atoms,q['roles'])):
            lines.append(f'w[{j}]=selected[{role}];if(w[{j}]==0xffffffff||r[w[{j}]*STRIDE+1])return false;')
            for col,x in enumerate(a['args']):
                value=f'r[w[{j}]*STRIDE+4+{col}]'
                if 'literal' in x:lines.append(f'if({value}!={x["literal"]}L)return false;')
                else:
                    slot=q['slots'][x['var']]
                    lines.append(f'if(v[{slot}]!={value})return false;' if slot in known else f'v[{slot}]={value};');known.add(slot)
        for a in q['rule_ast']['body']:
            if 'primitive' not in a['call']:continue
            def expr(x):return str(x['literal'])+'L' if 'literal' in x else f'v[{q["slots"][x["var"]]}]'
            op=a['call']['primitive'];lines.append(f'if(!({expr(a["args"][0])}{op}{expr(a["args"][1])}))return false;')
            if 'var' in a['args'][-1]:lines.append(f'v[{q["slots"][a["args"][-1]["var"]]}]=0;')
        cases.append(f'case {q["rule"]}:{{'+''.join(lines)+'return true;}')
    out.append('bool jp_bind(uint rule,thread uint*selected,device long*r,thread long*v,thread uint*w){switch(rule){'+''.join(cases)+'default:return false;}}')
    evals=[]
    for i,r in enumerate(rec):
        lines=[]
        def expr(k,x):return ('(-9223372036854775807L-1L)' if x==-(1<<63) else str(x)+'L') if k else f'v[{x}]'
        for j,(op,ak,a,bk,b) in enumerate(r['operations']):
            aa,bb=expr(ak,a),expr(bk,b);lines.append(f'if(!integer_valid({op},{aa},{bb}))return false;v[{3+j}]=integer_eval({op},{aa},{bb});values[{j}]=v[{3+j}];')
        k,x=r['result'];lines.append(f'cost={expr(k,x)};return true;');evals.append(f'case {i}:{{'+''.join(lines)+'}')
    out.append('bool jp_evaluate(uint recipe,thread long*v,thread long*values,thread long&cost){switch(recipe){'+''.join(evals)+'default:return false;}}')
    return '\n'.join(out)+'\n'

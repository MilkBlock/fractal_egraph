"""Compile fixed-carrier positive Saturate blocks to certified matrix programs.

Structural marker ports are read on the device. No CPU row matching or closure
truth is uploaded. Unsupported effect semantics retain the ordinary executor.
"""
import math,struct,hashlib,json


def separator(k):
    # Symmetry reduces ridge on the entire Boolean cube to two coefficients.
    p=math.ldexp(1.0,-k)
    if not p:raise ValueError('conjunction domain exceeds float certificate')
    if k:
        d=(1+p)*((k+1)/4+p)-k/4
        b=(p*(1+p)-p/2)/d;a=(p-k*b/2)/(1+p)
    else:a=p/(1+p);b=0
    scale=1/max(abs(a),abs(b));half=lambda x:struct.unpack('<e',struct.pack('<e',x))[0]
    a,b=half(a*scale),half(b*scale);positive=a+k*b;negative=positive-b if k else positive-1
    threshold=(positive+negative)/2
    # Inputs are exactly 0/1 and coefficients have already been rounded to FP16.
    # Bound FP32 accumulation and threshold rounding conservatively. The theorem
    # is about this finite Boolean domain, not arbitrary learned predictions.
    u=2**-23;n=2*(k+1)+8
    if n*u>=1:raise ValueError('uncertified accumulation width')
    error=n*u/(1-n*u)*(abs(a)+k*abs(b)+abs(threshold))
    if (k and b<=0) or min(positive-threshold,threshold-negative)<=error:raise ValueError('uncertified matrix separator')
    return {'arity':k,'bias':a,'coefficient':b,'threshold':threshold,'roundoff_bound':error,'margin':(positive-negative)/2}


def analyze(core,sets):
    tables={t['name']:t for t in core['tables']};programs=[];rejected=[]
    for sid,rs in enumerate(core['rulesets']):
        try:
            if not rs['rules']:raise ValueError('empty ruleset')
            output={a['call']['table'] for r in rs['rules'] for a in r['head'] if a['op']=='let' and 'table' in a.get('call',{})}
            if not output:raise ValueError('no predicate outputs')
            for name in output:
                t=tables[name]
                if t['kind']!='constructor' or len(t['schema'])!=2 or any(s['kind']!='eq' for s in t['schema']):raise ValueError('outputs require unary equality-key predicates')
            predicates=sorted(output);recipes=[]
            for rid,r in enumerate(rs['rules']):
                if any('table' not in a['call'] for a in r['body']):raise ValueError('guard/primitive requires a separate certificate')
                anchors=[a for a in r['body'] if a['call']['table'] not in output]
                if len(anchors)!=1:raise ValueError('requires one fixed structural anchor')
                anchor=anchors[0];t=tables[anchor['call']['table']]
                if any(s['kind']=='set' for s in t['schema']):raise ValueError('container port requires a separate certificate')
                columns={};aliases=[];literals=[]
                for i,a in enumerate(anchor['args']):
                    if 'literal' in a:literals.append((i,a))
                    elif a['var'] in columns:aliases.append((i,columns[a['var']]))
                    else:columns[a['var']]=i
                available=dict(columns);private=set();deps=[]
                for a in r['body']:
                    if a is anchor:continue
                    key,result=a['args']
                    if key.get('var') not in columns or result.get('var') in columns or result.get('var') in private or 'var' not in result:raise ValueError('nonlocal or constrained predicate binding')
                    private.add(result['var']);deps.append((predicates.index(a['call']['table']),columns[key['var']]))
                targets=[]
                for a in r['head']:
                    if a['op']=='copy':
                        if a['arg'].get('var') not in available:raise ValueError('RHS uses private predicate identity')
                        available[a['dst']]=available[a['arg']['var']]
                    elif a['op']=='let' and a.get('call',{}).get('table') in output and len(a['args'])==1 and a['args'][0].get('var') in available:
                        targets.append((predicates.index(a['call']['table']),available[a['args'][0]['var']]))
                    else:raise ValueError('effect changes structural carrier or visibility')
                recipes.append({'rule':sets[sid][rid],'anchor':anchor['call']['table'],'aliases':aliases,'literals':literals,'deps':deps,'targets':targets,'seminaive':r['seminaive']})
            for k in {len(r['deps']) for r in recipes}:separator(k)
            programs.append({'set':sid,'name':rs['name'],'predicates':predicates,'recipes':recipes,'rules_sha256':hashlib.sha256(json.dumps(rs['rules'],sort_keys=True).encode()).hexdigest()})
        except (ValueError,KeyError,OverflowError,ZeroDivisionError) as e:rejected.append({'set':sid,'name':rs['name'],'reason':str(e)})
    return programs,rejected


def lower(core,sets,table_ids,string_ids,enabled):
    programs,rejected=analyze(core,sets) if enabled else ([],[])
    arities=sorted({len(r['deps']) for p in programs for r in p['recipes']}) or [0]
    models=[separator(k) for k in arities];max_k=max(arities);max_out=max([len(r['targets']) for p in programs for r in p['recipes']]+[1])
    width=(max_k+1+7)//8*8;heads=(len(arities)+7)//8*8;weights=[0.0]*(width*heads)
    for h,model in enumerate(models):
        weights[h]=model['bias']
        for j in range(model['arity']):weights[(j+1)*heads+h]=model['coefficient']
    starts=[];predtables=[];setplans={p['set']:p for p in programs};cases=[]
    for sid in range(len(sets)):
        starts.append(len(predtables));p=setplans.get(sid)
        if p:predtables.extend(table_ids[n] for n in p['predicates'])
    starts.append(len(predtables))
    # The same row visitor either counts actual markers or fills their ports.
    for p in programs:
        bytable={}
        for r in p['recipes']:bytable.setdefault(r['anchor'],[]).append(r)
        tablecases=[]
        for name,recipes in bytable.items():
            body=[]
            for r in recipes:
                checks=[f'rows[row*STRIDE+4+{a}]==rows[row*STRIDE+4+{b}]' for a,b in r['aliases']]
                for c,a in r['literals']:
                    x=string_ids[a['literal']] if a['sort']['kind']=='String' else a['literal']
                    lit='(-9223372036854775807L-1L)' if x==-(1<<63) else str(x)+'L';checks.append(f'rows[row*STRIDE+4+{c}]=={lit}')
                code='if('+('&&'.join(checks) or 'true')+') { if(write){uint q=(offset+count)*MS_RECORD;'
                values=[str(len(r['deps'])),str(arities.index(len(r['deps']))),str(len(r['targets'])),str(r['rule']),str(int(r['seminaive'])),f'uint(rows[row*STRIDE+2])']
                values += [f'{pid}*ids+uint(rows[row*STRIDE+4+{c}])' for pid,c in r['deps']]+['0']*(max_k-len(r['deps']))
                values += [f'{pid}*ids+uint(rows[row*STRIDE+4+{c}])' for pid,c in r['targets']]+['0']*(max_out-len(r['targets']))
                code+=''.join(f'markers[q+{j}]={v};' for j,v in enumerate(values))+'}count++;}'
                body.append(code)
            tablecases.append(f'case {table_ids[name]}:'+'{'+''.join(body)+'break;}')
        cases.append(f'case {p["set"]}:switch(uint(rows[row*STRIDE]))'+'{'+''.join(tablecases)+'default:break;}break;')
    helper=f'#define MS_K {max_k}\n#define MS_RECORD {6+max_k+max_out}\n#define MS_WIDTH {width}\n#define MS_HEADS {heads}\n'
    for name,items in [('MS_START',starts),('MS_TABLES',predtables or [0])]:helper+='constant uint '+name+'[]={'+','.join(map(str,items))+'};\n'
    helper+='constant float MS_THRESHOLD[]={'+','.join(f'{m["threshold"]:.12g}f' if '.' in f'{m["threshold"]:.12g}' else f'float({m["threshold"]:.12g})' for m in models)+'};\n'
    helper+='uint ms_visit(uint set,uint row,uint ids,device long*rows,device uint*markers,uint offset,bool write){if(rows[row*STRIDE+1])return 0;uint count=0;switch(set){'+''.join(cases)+'default:break;}return count;}\n'
    return helper,{'enabled':enabled,'programs':programs,'rejected':rejected,'models':models,'width':width,'heads':heads,'record_stride':6+max_k+max_out,'weights':[struct.unpack('<H',struct.pack('<e',v))[0] for v in weights]}

"""Optional linear ordering of versioned query continuations.

Features use typed numeric values and aliases, never opaque IDs as numbers.
Scores only select the first/second exhaustive queue pass; no negative is
discarded. Each plan has a small ridge head with an explicit layout digest.
"""
import hashlib
import json
import math

def feature_layout(spec, sorts):
    features = [{'kind':'bias'}]
    for i, slot in enumerate(spec['slots']):
        sort = sorts[slot]
        if sort['kind'] == 'i64':
            features += [{'kind':kind,'a':i} for kind in ['sign','zero','magnitude']]
        elif sort['kind'] == 'bool':
            features.append({'kind':'bool','a':i})
        if sort['kind'] == 'eq':
            for j, other in enumerate(spec['slots'][:i]):
                if sorts[other] == sort:
                    features.append({'kind':'alias','a':i,'b':j})
    return features

def layout_digest(specs):
    return hashlib.sha256(json.dumps(specs,sort_keys=True,separators=(',',':')).encode()).hexdigest()

def lower(specs, model):
    digest = layout_digest(specs)
    width = max([len(s['features']) for s in specs]+[1])
    mode = 'none'
    coefficients = [[0.0]*width for _ in specs]
    if model is not None:
        if model.get('schema') != 'egglog-context-ridge/v1' or model.get('layout_sha256') != digest:
            raise ValueError('context ridge layout mismatch')
        coefficients = model['coefficients']
        if len(coefficients)!=len(specs) or any(len(w)!=width for w in coefficients):
            raise ValueError('context ridge coefficient shape')
        if any(not math.isfinite(x) or abs(x)>1e6 for w in coefficients for x in w):
            raise ValueError('invalid context ridge coefficients')
        mode = 'ridge'
    cases=[]
    for s in specs:
        expressions=[]
        for f in s['features']:
            a=f'contexts[ca+{8+f.get("a",0)}]'
            expr={'bias':'1.0f','sign':f'float(({a}>0)-({a}<0))',
                  'zero':f'float({a}==0)','bool':f'float({a}!=0)',
                  'magnitude':f'float(64-clz(({a}<0?0UL-ulong({a}):ulong({a}))))/64.0f',
                  'alias':f'float({a}==contexts[ca+{8+f.get("b",0)}])'}[f['kind']]
            expressions.append(expr)
        cases.append(f'case {s["id"]}:switch(feature)'+'{'+''.join(f'case {i}:return {e};' for i,e in enumerate(expressions))+'default:return 0.0f;}')
    code=f'#define CONTEXT_RIDGE {int(mode=="ridge")}\n#define CONTEXT_FEATURES {width}\n'
    code+='constant float CONTEXT_WEIGHTS[]={'+','.join(f'{float(v):.9e}f' for w in coefficients for v in w or [0.0])+'};\n' if specs else 'constant float CONTEXT_WEIGHTS[]={0.0f};\n'
    code+='float context_feature(uint plan,uint feature,uint ca,device long*contexts){switch(plan){'+''.join(cases)+'default:return 0.0f;}}\n'
    code+='float context_score(uint plan,uint ca,device long*contexts){float score=0;for(uint j=0;j<CONTEXT_FEATURES;j++)score+=context_feature(plan,j,ca,contexts)*CONTEXT_WEIGHTS[plan*CONTEXT_FEATURES+j];return score;}\n'
    return code, {'mode':mode,'layout_sha256':digest,'feature_width':width,'heads':len(specs)}

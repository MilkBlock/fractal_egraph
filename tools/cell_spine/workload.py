"""Runtime-only pricing for the existing typed spine/layout domain.

Private work is measured. Shared-prefix prices use measured streaming census
work and live cardinalities (unfiltered residuals), never products
of table sizes. The profile is performance evidence, not a matching certificate.
"""
import copy
import gzip
import hashlib
import json
from pathlib import Path
from . import shared_prefix


def canonical(v):return json.dumps(v,sort_keys=True,separators=(',',':'))
def shape_key(shape):
    shape=copy.deepcopy(shape);shape['slots']=[['',s] for _,s in shape['slots']]
    return shape

def query_key(q):return canonical([q['ruleset'],q['rule'],shape_key(q['shape'])])
def plan_key(plan):return canonical([shape_key(plan['shape']),plan['seed'],plan['steps']])
def digest(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def add(a,b):
    for k,v in b.items():a[k]=a.get(k,0)+v


def _read_profiles(tasks):
    queries={};plans={};prefixes={};cases=[];sources=[];rejected=[]
    for case,part in tasks:
        path=Path(part['profile'])
        if part['status']!='complete' or digest(path)!=part['profile_sha256'] or digest(part['input'])!=part['input_sha256']:raise ValueError('profile provenance differs')
        identity=[case['source'],part['name']];rounds=0;header=None;finished=False
        with gzip.open(path,'rt') as f:
            for line in f:
                r=json.loads(line)
                if 'header' in r:
                    header=r['header'];
                    if header['schema']!='marker-round-profile/v1':raise ValueError('unknown profile schema')
                    ids={};qids={};query_tables={};table_versions=[0]*len(header['tables']);versions={}
                    table_ids={name:i for i,name in enumerate(header['tables'])};prefix_defs={}
                    for qid,q in enumerate(header['queries']):
                        key=query_key(q);qids[qid]=key
                        query_tables[key]={table_ids[a['table']] for a in q['shape']['atoms']}
                        queries.setdefault(key,dict(ruleset=q['ruleset'],rule=q['rule'],shape=shape_key(q['shape']),requests=0,observations=0,empty_requests=0,nonempty_requests=0))
                    rejected.extend(dict(case=identity,rule=q.get('rule'),ruleset=q.get('ruleset'),reason=q.get('retained_native','unsupported column sort')) for q in header.get('rejected',[]))
                    for p in header['plans']:
                        key=canonical([qids[p['query']],plan_key(p['plan'])]);ids[p['id']]=key
                        base=p['plan'];order=[base['seed']]+[i for i,_ in base['steps']]
                        prefix_defs[p['id']]=[(canonical(json.loads(body)),tuple(sorted({table_ids[base['shape']['atoms'][i]['table']] for i in order[:depth+1]}))) for depth,body in enumerate(p['prefix_keys'])]
                        plans.setdefault(key,dict(query=qids[p['query']],plan=p['plan'],root_table=p['plan']['shape']['atoms'][p['plan']['seed']]['table'],step_tables=[p['plan']['shape']['atoms'][i]['table'] for i,_ in p['plan']['steps']],requests=0,root_evaluations=0,index_reads=0,candidate_rows=0,bindings=0,shared_prefix_uses=0,
                            steps=[{} for _ in p['plan']['steps']],census=[{} for _ in p['plan']['steps']],
                            live_prefixes=[0]*(len(p['plan']['steps'])+1),cases=[]))
                    continue
                if r.get('complete') is True:finished=True;continue
                if 'unmeasured' in r:raise ValueError('unmeasured Run boundary: '+r['unmeasured'])
                if not r.get('verified') or header is None:raise ValueError('unverified/missing round header')
                rounds+=1
                live_tables={t:live for t,live,_ in r['tables']}
                for t,live,updates in r['tables']:
                    table_versions[t]+=updates
                observations={x[0]:x[1:] for x in r['plans']};active_queries=set()
                for stage in r['stages']:
                    pid=stage['plan'];p=plans[ids[pid]];active_queries.add(p['query'])
                    p['requests']+=1
                    if identity not in p['cases']:p['cases'].append(identity)
                    for field,value in zip(['root_evaluations','index_reads','candidate_rows','bindings','shared_prefix_uses'],observations[pid]):p[field]=p.get(field,0)+value
                    for dst,src in zip(p['steps'],stage['steps']):add(dst,src)
                    for dst,src in zip(p['census'],stage['prefix_census']):add(dst,src)
                    p['live_prefixes']=[max(a,b) for a,b in zip(p['live_prefixes'],stage['live_prefixes'])]
                    work=observations[pid][0]
                    for depth,(body,tables) in enumerate(prefix_defs[pid]):
                        version=tuple(table_versions[t] for t in tables)
                        # All-root trials may observe the same prefix more
                        # than once. Coalesce same-snapshot demands; changed
                        # table versions remain separate maintenance work.
                        if depth:
                            s=stage['prefix_census'][depth-1];work+=s.get('lookups',0)+s.get('rows',0)
                        previous=versions.get(body)
                        old=previous[1] if previous and previous[0]==version else 0
                        versions[body]=(version,max(old,work))
                        shared=prefixes.setdefault(body,dict(update_work=0,peak_bindings=0,cases=[]))
                        shared['update_work']+=max(0,work-old)
                        shared['peak_bindings']=max(shared['peak_bindings'],stage['live_prefixes'][depth])
                        if identity not in shared['cases']:shared['cases'].append(identity)
                for key in active_queries:
                    q=queries[key];q['requests']+=1;q['observations']+=1
                    q['empty_requests' if any(live_tables[t]==0 for t in query_tables[key]) else 'nonempty_requests']+=1
        if not finished:raise ValueError('truncated profile')
        cases.append(dict(case=identity,rounds=rounds));sources.append(dict(path=str(path),sha256=part['profile_sha256'],input_sha256=part['input_sha256']))
    return dict(queries=queries,plans=plans,prefixes=prefixes,cases=cases,sources=sources,rejected=rejected)


def _read_pass(task):return _read_profiles([task])


def _merge_profiles(dst,src):
    for key,q in src['queries'].items():
        if key not in dst['queries']:dst['queries'][key]=q
        else:
            for f in ['requests','observations','empty_requests','nonempty_requests']:dst['queries'][key][f]+=q[f]
    for key,p in src['plans'].items():
        if key not in dst['plans']:dst['plans'][key]=p;continue
        d=dst['plans'][key]
        for f in ['requests','root_evaluations','index_reads','candidate_rows','bindings','shared_prefix_uses']:d[f]=d.get(f,0)+p.get(f,0)
        for field in ['steps','census']:
            for a,b in zip(d[field],p[field]):add(a,b)
        d['live_prefixes']=[max(a,b) for a,b in zip(d['live_prefixes'],p['live_prefixes'])]
        d['cases'].extend(c for c in p['cases'] if c not in d['cases'])
    for key,p in src['prefixes'].items():
        if key not in dst['prefixes']:dst['prefixes'][key]=p;continue
        d=dst['prefixes'][key];d['update_work']+=p['update_work'];d['peak_bindings']=max(d['peak_bindings'],p['peak_bindings'])
        d['cases'].extend(c for c in p['cases'] if c not in d['cases'])
    for field in ['cases','sources','rejected']:dst[field].extend(src[field])


def collect(manifest,jobs=1):
    manifest=Path(manifest);m=json.loads(manifest.read_text())
    if m.get('schema')!='marker-workload-corpus/v1' or not m.get('complete'):
        raise ValueError('complete verified corpus required; failed/missing cases are not zero cost')
    expected={x['source']:x['sha256'] for x in m['identity']['workloads']}
    if len(m['cases'])!=len(expected) or {c['source'] for c in m['cases']}!=set(expected) or any(c['status']!='complete' or c['source_sha256']!=expected[c['source']] for c in m['cases']):
        raise ValueError('corpus case coverage/provenance incomplete')
    if jobs<1:raise ValueError('jobs must be positive')
    tasks=[(case,part) for case in m['cases'] for part in case['passes']]
    if jobs==1:result=_read_profiles(tasks)
    else:
        from concurrent.futures import ProcessPoolExecutor
        result=_read_profiles([])
        with ProcessPoolExecutor(max_workers=jobs) as pool:
            # Preserve manifest order; integer sums/maxima are identical to the
            # serial reader, independent of worker completion order.
            for part in pool.map(_read_pass,tasks):_merge_profiles(result,part)
    queries=result['queries'];plans=result['plans'];prefixes=result['prefixes']
    cases=result['cases'];sources=result['sources'];rejected=result['rejected']
    return dict(schema='marker-runtime-workload/v1',corpus_sha256=digest(manifest),corpus=str(manifest),
        cases=cases,sources=sources,queries=queries,plans=plans,prefixes=prefixes,rejected=rejected,
        summary=dict(workloads=len(m['cases']),passes=len(cases),run_boundaries=sum(c['rounds'] for c in cases),
            admitted_query_shapes=len(queries),observed_query_shapes=sum(q['requests']>0 for q in queries.values()),
            alternative_plans=len(plans),observed_plans=sum(p['requests']>0 for p in plans.values()),
            structural_prefixes=len(prefixes),rejected_rules=len({(q['ruleset'],q['rule'],q['reason']) for q in rejected})),
        objective='sum of measured work over every workload/pass/Run; peak runtime cardinalities; no initial-table or Cartesian fanout estimates',
        limits='shared maintenance coalesces same-version census work by maximum; exact union of consumer dirty-root sets is not recorded, so this is an estimate rather than a proven bound; not measured wall time or a new closure proof')


def load(path):
    r=json.loads(Path(path).read_text())
    if r.get('schema')!='marker-runtime-workload/v1':raise ValueError('unknown runtime workload')
    if not r.get('cases') or not r.get('sources'):raise ValueError('empty runtime workload')
    return r


def options(q,workload):
    from . import scheduled
    qkey=query_key(q)
    if qkey not in workload['queries']:return []
    observation=workload['queries'][qkey]
    # The ordinary GPU/native executor already short-circuits an empty input.
    # Do not replace that zero-binding work by a newly maintained context.
    if observation.get('nonempty_requests')==0:return []
    measured_plans={canonical([p['seed'],p['steps']]):workload['plans'].get(canonical([qkey,plan_key(p)])) for p in q['plans']}
    result=[];seen=set();descriptions={}
    def describe(plan):
        key=(plan['seed'],canonical(plan['steps']),plan['prefix_len'],tuple(plan['carried_slots']))
        if key not in descriptions:descriptions[key]=shared_prefix.describe(plan)
        return descriptions[key]
    # Only the structural domain is enumerated here. Discard every static price.
    for o in scheduled.options(q,{}):
        order=canonical([o['plan']['seed'],o['plan']['steps']])
        measured=measured_plans[order]
        if not measured or measured['query']!=qkey or not measured['requests']:continue
        if measured['root_evaluations']==0 and measured['candidate_rows']==0:continue
        depth=o['plan']['prefix_len'];shape=o['plan']['shape']
        identity=(order,depth,tuple(o['plan']['carried_slots']))
        if identity in seen:continue
        seen.add(identity)
        if depth==0:
            private=dict(o,plan=dict(o['plan'],carried_slots=[],cache_ports=True),stored_slots=[],shared=None)
            private['indexes']=[]  # measured read/index work already includes these accesses
            roots=measured['root_evaluations']
            # PortPath has one owner-row handle and at most one saved read per
            # step. These are storage/work estimates, not measured bytes/time.
            path_fields=measured['live_prefixes'][0]*(1+len(measured['steps'])) if measured['steps'] else 0
            path_writes=sum(min(roots,s.get('lookups',0)) for s in measured['steps'])
            private['costs']=dict(search=measured['root_evaluations']+sum(sum(s.get(k,0) for k in ('entries','rows','lookups')) for s in measured['steps'])+measured['bindings'],
                storage=path_fields,update=path_writes)
            private['cost_evidence']=dict(kind='measured_private_with_path_estimate',cases=measured['cases'],requests=measured['requests'],
                path_fields_bound=path_fields,path_writes_bound=path_writes)
            result.append(private)
            # Without saved ports, each entered stage needs a lookup. Its
            # terminal delta optimization may visit fewer old rows, so keeping
            # observed visits is an estimate, not an exact no-cache measurement.
            lost_reads=sum(max(0,s.get('entries',0)-s.get('lookups',0)) for s in measured['steps'])
            result.append(dict(private,plan=dict(private['plan'],cache_ports=False),
                costs=dict(storage=0,update=0,search=private['costs']['search']+lost_reads),
                cost_evidence=dict(kind='measured_stages_no_path_estimate',cases=measured['cases'],requests=measured['requests'],
                    additional_lookups=lost_reads,limits='terminal stale-row pruning may lower no-cache work')))
        requirements=[]
        for d in range(depth+1):
            p=dict(o['plan'],prefix_len=d)
            if d<depth:p['carried_slots']=list(range(len(shape['slots'])))
            desc=describe(p);r=workload['prefixes'].get(desc['body_key'])
            if not r:break
            missing=len(desc['slots'])-len(desc['carried'])
            requirements.append(dict(definition=desc,costs=dict(storage=r['peak_bindings']*(len(desc['carried'])+(2 if missing else 0)),update=r['update_work']),
                evidence=dict(basis='runtime prefix census; no table-size product',prefix_bindings=r['peak_bindings'],measured=True)))
        if len(requirements)!=depth+1:continue
        desc=requirements[-1]['definition']
        frames=measured['root_evaluations'] if depth==0 else measured['census'][depth-1].get('prefixes',0)
        decode=len(desc['slots'])+(len(desc['body']['atoms'][-1]['columns']) if len(desc['slots'])>len(desc['carried']) else 0)
        residual=sum(sum(s.get(k,0) for k in ('entries','rows','lookups')) for s in measured['census'][depth:])
        shared=dict(o,plan=dict(o['plan'],shared_marker=True),shared=desc,required_markers=requirements,indexes=[],
            costs=dict(storage=0,update=0,search=frames*decode+residual+measured['bindings']),
            cost_evidence=dict(kind='measured_census_estimate',cases=measured['cases'],requests=measured['requests']))
        result.append(shared)
    return list({(o['plan']['seed'],canonical(o['plan']['steps']),o['plan']['prefix_len'],tuple(o['plan']['carried_slots']),bool(o.get('shared')),o['plan'].get('cache_ports',True)):o for o in result}.values())

"""Prepare real one-hop lookup demands shared by many eggcc rules.

Uses an existing native query-shape header and saved canonical fact tables.
Only a single seed atom is read/filtered to create partial bindings. No target
lookup, full rule match, guard or RHS execution is done to create requests.
Alternative admissible seeds are included; this is NOT a measured Run stream.
"""
import argparse
from collections import defaultdict
import gzip
import hashlib
import json
from pathlib import Path
import struct
import numpy as np
from field import PRIME,PolynomialIndex,hash_keys


def value(v):
    kind=next(iter(v));sort,item=v[kind]
    return kind,sort,str(item)


def demands(snapshot,profile,target):
    with gzip.open(profile,'rt') as f:header=json.loads(next(f))['header']
    selected=[];needed={target}
    for qi,q in enumerate(header['queries']):
        seen=set()
        for plan in q['plans']:
            if not plan['steps']:continue
            seed=plan['seed'];next_atom=plan['steps'][0][0]
            if q['shape']['atoms'][next_atom]['table']!=target:continue
            if (seed,next_atom) in seen:continue
            seen.add((seed,next_atom));selected.append((qi,q,seed,next_atom))
            needed.add(q['shape']['atoms'][seed]['table'])
    saved=json.loads(snapshot.read_text());tables=defaultdict(list)
    for op,args,result,hidden in saved['rows']:
        if op in needed and not hidden:tables[op].append(tuple(value(x) for x in [*args,result]))
    del saved
    if not tables[target]:raise ValueError('empty target table')
    arity=len(tables[target][0]);sorts=[v[1] for v in tables[target][0]]
    def code(col,v):
        if v[1]!=sorts[col]:raise ValueError('source/target sort mismatch')
        if v[0]!='Eq' and v[1]!='i64':raise ValueError('this prototype only encodes native equality IDs and exact i64')
        return int(v[2])&((1<<64)-1)
    encoded=np.array([[code(c,v) for c,v in enumerate(row)] for row in tables[target]],dtype=np.uint64)
    keys_by_mask=defaultdict(set);rules_by_mask=defaultdict(set);requests=defaultdict(int)
    for qi,q,seed,next_atom in selected:
        a=q['shape']['atoms'][seed];b=q['shape']['atoms'][next_atom]
        for row in tables[a['table']]:
            binding={};valid=True
            for column,sort,v in zip(a['columns'],a['sorts'],row,strict=True):
                if 'Literal' in column:
                    if v!=('Base',sort,column['Literal']):valid=False;break
                else:
                    slot=column['Slot']
                    if slot in binding and binding[slot]!=v:valid=False;break
                    binding[slot]=v
            if not valid:continue
            mask=0;key=[0]*arity
            for col,(column,sort) in enumerate(zip(b['columns'],b['sorts'],strict=True)):
                v=('Base',sort,column['Literal']) if 'Literal' in column else binding.get(column['Slot'])
                if v is None:continue
                mask|=1<<col;key[col]=code(col,v)
            if not mask:continue
            keys_by_mask[mask].add(tuple([mask,*key]));rules_by_mask[mask].add(qi);requests[mask]+=1
    return encoded,keys_by_mask,{'queries_in_header':len(header['queries']),
        'rejected_queries_in_header':len(header['rejected']),'source_seed_plans':len(selected),
        'target':target,'rows':len(encoded),'arity':arity,
        'groups':[{'mask':m,'rules':len(rules_by_mask[m]),'root_generated_requests':requests[m],
                   'distinct_requests':len(keys_by_mask[m])} for m in sorted(keys_by_mask)],
        'rules':{str(m):[{'id':i,'ruleset':header['queries'][i]['ruleset'],'rule':header['queries'][i]['rule']}
                        for i in sorted(rules_by_mask[m])] for m in sorted(rules_by_mask)}}


def build_bank(rows,probes,page_rows):
    if not 0<page_rows<512:raise ValueError('page rows must be in 1..511')
    if not len(rows) or not len(probes):raise ValueError('empty rows or probes')
    mask=int(probes[0,0]);arity=rows.shape[1]
    projected=np.zeros((len(rows),arity+1),dtype=np.uint64);projected[:,0]=mask
    for c in range(arity):
        if mask&(1<<c):projected[:,c+1]=rows[:,c]
    hashes=hash_keys(projected);queries=hash_keys(probes)
    pages=[];payloads=[];hash_tables=[];bits=[];degrees=[]
    capacity=1
    while capacity<2*page_rows:capacity*=2
    for start in range(0,len(rows),page_rows):
        h=hashes[start:start+page_rows];index=PolynomialIndex(h[:,0]);p2=PolynomialIndex(h[:,1]).p
        d=max(len(index.p),len(index.f),len(p2));degrees.append(d-1)
        pages.append((index.p,index.f,p2))
        buckets=[[] for _ in index.handles]
        for i,x in enumerate(h[:,0]):buckets[index.handles[int(x)]].append(start+i)
        payloads.append(buckets)
        table=np.zeros(capacity,dtype=np.uint32)
        for key,handle in index.handles.items():
            slot=(key*2654435761)&(capacity-1)
            while table[slot]:slot=(slot+1)&(capacity-1)
            table[slot]=((handle+1)<<16)|(key+1)
        hash_tables.append(table)
        words=np.zeros((PRIME+31)//32,dtype=np.uint32)
        for root in set(map(int,h[:,1])):words[root//32]|=np.uint32(1)<<np.uint32(root%32)
        bits.append(words)
    width=((max(degrees)+1+31)//32)*32
    if width>512:raise ValueError('this worker supports <=512 coefficients; no truncated roots')
    coeff=np.zeros((3,len(pages),width),dtype=np.uint32)
    for i,polys in enumerate(pages):
        for j,p in enumerate(polys):coeff[j,i,:len(p)]=p
    return coeff,queries,np.array(hash_tables),np.array(bits),payloads,projected,degrees


def write_packet(path,coeff,queries,tables,bits):
    _,m,k=coeff.shape;p=len(queries);h=tables.shape[1];w=bits.shape[1]
    with path.open('wb') as f:
        f.write(b'POLYKEY1');f.write(struct.pack('<6I',m,k,p,h,w,PRIME))
        for data in (coeff,queries,tables,bits):f.write(np.asarray(data,dtype='<u4').tobytes())


def main():
    p=argparse.ArgumentParser();p.add_argument('--snapshot',type=Path,required=True)
    p.add_argument('--profile',type=Path,required=True);p.add_argument('--table',default='TCPair')
    p.add_argument('--mask',type=int,default=4);p.add_argument('--page-rows',type=int,default=127)
    p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    if not 0<a.page_rows<512:p.error('--page-rows must be in 1..511')
    a.output.mkdir(parents=True,exist_ok=False)
    rows,keys,meta=demands(a.snapshot,a.profile,a.table)
    if a.mask not in keys:raise ValueError(f'available masks: {list(keys)}')
    probes=np.array(sorted(keys[a.mask]),dtype=np.uint64)
    coeff,queries,table,bits,payload,projected,degrees=build_bank(rows,probes,a.page_rows)
    write_packet(a.output/'input.bin',coeff,queries,table,bits)
    np.savez(a.output/'verify.npz',rows=projected,probes=probes)
    (a.output/'payloads.json').write_text(json.dumps(payload))
    meta.update({'snapshot_sha256':hashlib.sha256(a.snapshot.read_bytes()).hexdigest(),
                 'profile_header_source':str(a.profile),'profile_sha256':hashlib.sha256(a.profile.read_bytes()).hexdigest(),
                 'mask_evaluated':a.mask,'page_rows':a.page_rows,'pages':len(payload),
                 'coefficients':coeff.shape[2],'degree_min':min(degrees),'degree_max':max(degrees),
                 'coefficient_nonzero_fraction':float((coeff!=0).mean()),
                 'filter_payload_bytes':int(coeff.nbytes),'hash_bitmap_bytes':int(table.nbytes+bits.nbytes),
                 'query_points':len(probes),'logical_matrix_entries':len(probes)*len(payload),
                 'scope':'deduplicated first-step partial lookups from admitted source seed plans on frozen tables; not actual per-Run requests or complete rule matches'})
    (a.output/'manifest.json').write_text(json.dumps(meta,indent=2)+'\n')
    print(json.dumps({k:v for k,v in meta.items() if k!='rules'},indent=2))


if __name__=='__main__':main()

"""Verify every returned bucket against original keys and an exact lookup index."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import numpy as np


def verify(directory,prefix):
    meta=json.loads((directory/'manifest.json').read_text());gpu=json.loads(prefix.with_suffix('.json').read_text())
    saved=np.load(directory/'verify.npz');rows=saved['rows'];probes=saved['probes']
    payload=json.loads((directory/'payloads.json').read_text())
    result=np.fromfile(prefix.with_suffix('.bin'),dtype='<u2').reshape(meta['pages'],meta['query_points'])
    index=defaultdict(list)
    for i,row in enumerate(rows):index[tuple(map(int,row))].append(i)
    expected={(q,r) for q,key in enumerate(probes) for r in index.get(tuple(map(int,key)),())}
    actual=set();false_pages=0;verified_records=0
    for page,q in np.argwhere(result):
        handle=int(result[page,q])-1
        if not 0<=handle<len(payload[page]):raise AssertionError('invalid payload index')
        hit=False
        for r in payload[page][handle]:
            verified_records+=1
            if np.array_equal(rows[r],probes[q]):actual.add((int(q),r));hit=True
        if not hit:false_pages+=1
    assert actual==expected, f'missing {len(expected-actual)}, extra {len(actual-expected)}'
    candidates=int(np.count_nonzero(result))
    throughput=gpu['polynomial_float_operations']/gpu['polynomial_gpu_seconds']
    reference=gpu['reference_gemm_float_operations']/gpu['reference_gemm_seconds']
    report={'scope':meta['scope'],'lookup_indices_exact_after_bucket_check':True,
            'expected_row_indices':len(expected),'returned_candidate_pages':candidates,
            'false_candidate_pages':false_pages,'candidate_page_precision':(candidates-false_pages)/candidates if candidates else None,
            'cpu_bucket_row_checks':verified_records,'no_missing_matching_rows':True,
            'matrix_to_reference_gemm_throughput':throughput/reference,
            'matrix_gpu_vs_same_filter_hash_ratio':gpu['polynomial_gpu_seconds']/gpu['hash_gpu_seconds'],
            'matrix_effective_tflops':throughput/1e12,'reference_gemm_tflops':reference/1e12,
            'input_sha256':hashlib.sha256((directory/'input.bin').read_bytes()).hexdigest(),
            'gpu':gpu,'manifest':{k:v for k,v in meta.items() if k!='rules'},
            'warning':'page filtering is more work than a global exact hash lookup; dense arithmetic throughput is not inference speedup'}
    output=prefix.parent/(prefix.name+'-verified.json');output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k not in ('gpu','manifest')},indent=2))


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--data',type=Path,required=True);p.add_argument('--prefix',type=Path,required=True)
    a=p.parse_args();verify(a.data,a.prefix)

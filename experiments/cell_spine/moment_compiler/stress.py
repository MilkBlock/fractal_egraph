"""Larger synthetic projection batch, not an egglog or end-to-end benchmark."""
import argparse
from collections import Counter
import json
from pathlib import Path
import random
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from experiments.cell_spine.moment_compiler.compiler import Circuit,bit_code,certify_commutative,layout


def run(bake,gpu_binary,out,count=512,bits=32,half=False):
    out.mkdir(parents=True,exist_ok=False)
    library=json.loads((bake/'library.json').read_text())
    ast=json.loads((bake/'rules.json').read_text())
    c=Circuit(bits,4,certify_commutative(ast,'Add'))
    types=[i%2 for i in range(count)]
    plan=layout(c,library,types,shared=True)
    rng=random.Random(73)
    if bits<16:raise ValueError('stress domain uses >=16 bits')
    pool=rng.sample(range(1<<bits),count*2+128)
    # Shared factors are synthetic. Never infer them or group by their values.
    bindings=[[pool[128+i],pool[(i%64)*2],pool[(i%64)*2+1],pool[128+count+i]] for i in range(count)]
    ports=[[bit_code(v,bits) for v in values] for values in bindings]
    payload=dict(circuit=c.serialize(),ports=ports,jobs=[dict(name='synthetic-shared',plan=plan)],half_inputs=half)
    (out/'input.json').write_text(json.dumps(payload)+'\n')
    subprocess.run([str(gpu_binary),str(out/'input.json'),str(out/'gpu.json')],check=True)
    raw=json.loads((out/'gpu.json').read_text())
    result=raw['results'][0]
    # Independent structural-key oracle; never used to generate the program,
    # group numerical rows, or perform GPU matching.
    terms={}
    for t in library['templates']:
        for ch in t['channels']:
            _,node=c.compile(ch['k']);terms[node]=ch['k']
    def key(t,values):
        if t[0]=='Input':return ('I',values[t[1]])
        return ('A',*sorted(key(x,values) for x in t[1:]))
    for q,output in zip(plan['queries'],result['outputs']):
        keys=[key(terms[n],bindings[i]) for i,n in q['wires']]
        expected=[int(a==b) for a in keys for b in keys]
        assert expected==output
    tapes=[c.evaluate(p)[0] for p in ports]
    assert result['feature_values']==[x for tape in tapes for row in tape for x in row]
    report=dict(scope='one synthetic operator batch; not native egglog work or network throughput comparison',
        count=count,bits=bits,device=raw['device'],backend=raw['backend'],plan=plan['stats'],
        blocks=[dict(rows=len(b['wires']),width=b['width']) for b in plan['blocks']],
        all_bits_equal=True,all_feature_values_equal=True,
        successful_condition_pairs=sum(sum(row) for row in result['outputs']),
        gpu_seconds=result['gpu_seconds'],submit_wait_seconds=result['submit_wait_seconds'],
        logical_matrix_gflop_per_second=2*plan['stats']['matrix_macs']/result['gpu_seconds']/1e9,
        timing_note='single first dispatch; includes feature/projection/distance kernels; excludes Swift/JSON compilation/readback; no inference about production throughput')
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    return report


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--bake',type=Path,required=True)
    p.add_argument('--gpu-binary',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--count',type=int,default=512)
    p.add_argument('--bits',type=int,default=32)
    p.add_argument('--half-inputs',action='store_true')
    a=p.parse_args();print(json.dumps(run(a.bake.resolve(),a.gpu_binary.resolve(),a.output.resolve(),a.count,a.bits,a.half_inputs),indent=2))

#!/usr/bin/env python3
"""Import existing Bake/spine/marker/transition experiments into the shared Rust
matrix CLI, then compare complete channels and target contexts with native egglog.
No new rule-family detector and no runtime invocation of the old matcher.
"""
import argparse,copy,hashlib,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from experiments.cell_spine.tensor_interaction import demo as tensor
from experiments.cell_spine.layered_context import demo as layers
from experiments.cell_spine.moment_compiler.compiler import bit_code
from experiments.cell_spine.transition_program import compiler as tc,demo as td


from tools.cell_spine.matrix_program import root_program,transition_program

def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--output',type=Path,required=True);ap.add_argument('--skip-build',action='store_true');a=ap.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    binary=ROOT/'target/cell-assisted/debug/cell-assisted-egglog';worker=out/'matrix-metal';native=ROOT/'target/analyzer/release/examples/native_math_baseline';parser=ROOT/'target/analyzer/release/examples/lhs_ast'
    if not a.skip_build:
        subprocess.run(['cargo','build','--offline','--manifest-path','cell-assisted-egglog/Cargo.toml','--target-dir','target/cell-assisted'],cwd=ROOT,check=True)
        subprocess.run(['cargo','build','--offline','--release','--example','lhs_ast','--example','native_math_baseline','--target-dir','target/analyzer'],cwd=ROOT,check=True)
    subprocess.run(['swiftc','-O',ROOT/'cell-assisted-egglog/matrix.swift','-o',worker],check=True)
    report=dict(scope='integration of existing bounded Bake programs into shared Rust IR and Metal/MPS executor; no general egglog speedup or FractalComb discovery claim',cases=[],native=[],native_binary_sha256=hashlib.sha256(native.read_bytes()).hexdigest(),runtime_binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),worker_sha256=hashlib.sha256(worker.read_bytes()).hexdigest())
    def run(name,spec):
        path=out/f'{name}.json';path.write_text(json.dumps(spec));results={}
        for backend in ['cpu','metal']:
            dest=out/f'{name}-{backend}.json'
            subprocess.run([binary,'--mode','matrix','--program',path,'--backend',backend,'--output',dest,'--gpu-worker',worker],cwd=ROOT,check=True,stdout=subprocess.DEVNULL)
            results[backend]=json.loads(dest.read_text())
        assert results['cpu']['epochs']==results['metal']['epochs'],(name,'CPU/GPU report mismatch')
        gpu=results['metal'];work=[l['work'] for e in gpu['epochs'] for s in e for l in s['levels']]
        report['cases'].append(dict(name=name,exact_cpu_gpu=True,epochs=len(gpu['epochs']),matrix_macs=sum(w['matrix_macs'] for w in work),matrix_calls=sum(w['matrix_calls'] for w in work),computed_features=sum(w['computed_features'] for w in work),reused_features=sum(w['reused_features'] for w in work),feature_components_written=sum(w['feature_components_written'] for w in work),feature_adds=sum(w['feature_adds'] for w in work),feature_multiplies=sum(w['feature_multiplies'] for w in work),gpu_seconds=gpu['gpu_seconds'],host_count_reads=gpu['host_count_reads'],matrix_bytes=gpu['matrix_bytes'],intermediate_binding_readbacks=gpu['intermediate_binding_readbacks']))
        (out/'report.json').write_text(json.dumps(report,indent=2))
        print('verified',name,flush=True);return gpu
    library=tensor.bake(out/'bake',native,parser=parser);prepared=layers.prepare(library,out/'bake');ast=json.loads((out/'bake/rules.json').read_text());book=tensor.Codebook(tensor.ALPHABET);old_cells=[book.encode(t,b,library['templates'][t]['arity'],library['width']) for t,b in tensor.CASES]
    for mode in ['root','all']:
        reference=layers.staged(library,prepared,old_cells,mode)
        for name,marker,eager in [('eager',False,True),('staged',False,False),('marker',True,False)]:
            spec,channels,ports=root_program(library,prepared,old_cells,mode,marker,eager,ast)
            result=run(f'{mode}-{name}',spec);states=result['epochs'][0][0]['decisions'];n=len(channels)
            gate=[[int(states[i*n+j]==1) for j in range(n)] for i in range(n)]
            assert gate==reference['gate'],(mode,name,'legacy full binding channels differ')
        if mode=='all':
            hits=tensor.native_oracle(out/'native-matrix-audit',native,library,old_cells,book,reference)
            report['native'].append(dict(kind='root and non-root full factoring bindings',bindings=len(hits),exact=True))
    source=(ROOT/'experiments/cell_spine/transition_program/rules.egg').read_text();rules=out/'transition-rules.json';subprocess.run([parser,ROOT/'experiments/cell_spine/transition_program/rules.egg',rules],check=True)
    bake=tc.Bake(out/'transition-bake',native,source);p=td.example(bake,json.loads(rules.read_text()));c,jobs=p.lower();p.c=c
    ports=[];active=[]
    for inst in p.instances:
        values=inst['binding'] or [];ports.append([bit_code(v,c.bits) for v in values]+[bit_code(0,c.bits)]*(c.slots-len(values)));active.append(int(inst['initial']))
    spec=transition_program(p,c,jobs,ports,active);result=run('transitions',spec);expected=td.reference(c,jobs,ports,active)
    actual=[]
    for si,s in enumerate(result['epochs'][0]):
        assert s['target_status']==expected[si]['transition_status']
        assert [int(v==1) for v in s['live']]==expected[si]['cell_valid']
        encoded=[v for cell in s['cells'] for x in cell for v in bit_code(x,c.bits)]
        assert encoded==expected[si]['cell_values']
        actual.append(dict(expected[si],cell_values=encoded,cell_valid=[int(v==1) for v in s['live']],transition_status=s['target_status']))
    audit=td.native_audit(out/'transition-native-audit',native,source,p,jobs,actual);report['native'].append(audit)
    updated=copy.deepcopy(spec);updated['updates']=[[],[dict(cell=0,slot=2,value=15)],[dict(cell=0,slot=0,value=14)]];incremental=run('transitions-incremental',updated)
    assert all(l['work']['computed_features']==l['work']['matrix_macs']==0 for s in incremental['epochs'][1] for l in s['levels'])
    final=copy.deepcopy(spec);final['cells'][0]['binding'][2]=15;final['cells'][0]['binding'][0]=14;fresh=run('transitions-fresh-update',final)
    for a,b in zip(incremental['epochs'][-1],fresh['epochs'][0]):
        for field in ['decisions','live','target_status']:assert a[field]==b[field],field
        # Inactive coordinates may retain old cached bytes; they are not published Cells.
        for i,live in enumerate(a['live']):
            if live==1:assert a['cells'][i]==b['cells'][i],('active cell',i)
    report['incremental_matches_fresh']=True
    fresh_ports=[[bit_code(v,c.bits) for v in (cell['binding'] or [])]+[bit_code(0,c.bits)]*(c.slots-len(cell['binding'] or [])) for cell in final['cells']]
    fresh_expected=td.reference(c,jobs,fresh_ports,active)
    for si,state in enumerate(fresh['epochs'][0]):
        assert state['target_status']==fresh_expected[si]['transition_status']
        assert [int(v==1) for v in state['live']]==fresh_expected[si]['cell_valid']
        assert [v for cell in state['cells'] for x in cell for v in bit_code(x,c.bits)]==fresh_expected[si]['cell_values']
    report['updated_native_audit']=td.native_audit(out/'updated-native-audit',native,source,p,jobs,fresh_expected)
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(out/'report.json')
if __name__=='__main__':main()

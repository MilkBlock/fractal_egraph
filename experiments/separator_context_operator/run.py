import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import numpy as np
from data import load_snapshot,fixture,unstructured_fixture,crossed_if_fixture
from model import (loop_factors,loop_effects,loop_reference,new_const_keys,
                   if_projection,if_masked_projection,if_reference,comparator_projection,comparator_reference,
                   signed_join,signed_remap,hadamard,delta_terms,delta_effects,delta_reference)
from model import fused_comparator_matrix
from native import check_loop,check_if,check_comparator


HERE=Path(__file__).resolve().parent


def loop_check(data):
    f=loop_factors(data);answer=loop_effects(data,f);reference=loop_reference(data)
    assert answer==reference
    constructors={(c,t,ctx) for root,ctx,t,c,old in answer}
    assert new_const_keys(f)==constructors
    k=f['numeric']['width'];raw=f['x']@f['y'].T//k
    assert np.array_equal(raw>0,(f['x']@f['y'].T)>0)
    report={'root_rows':len(data['loops']),'const_rows':len(data['constants']),
            'context_axis':len(f['ctx_ids']),'type_axis':len(f['type_ids']),
            'value_axis':len(f['value_ids']),'separator_width':k,
            'signed_left_density':float(np.mean(f['x']!=0)),
            'signed_right_density':float(np.mean(f['y']!=0)),
            'compatible_pair_density':float(f['compatible'].mean()),
            'rhs_coordinate_count':len(answer),'distinct_new_Const_keys':len(constructors),
            'new_Const_matrix_shape':list(f['new_const'].shape),
            'new_Const_density':float(f['new_const'].mean()),
            'full_binding_not_materialized_until_output':True,
            'raw_table_dense_cells':f['raw_table_cells'],'retained_factor_cells':f['latent_cells'],
            'verified_against_indexed_query':True,'numeric':f['numeric']}
    return report,f,answer


def matrix_packet(f):
    # Three dense contractions: context compatibility, project old Const rows
    # to their value coordinate, then share new Const construction across roots.
    return {'x':f['x'].tolist(),'y':f['y'].tolist(),
            'left':f['left'].tolist(),'types':f['types'].tolist(),
            'value_map':f['const_value'].tolist(),'normalization':f['numeric']['width']}


def term_packet(full,term,index):
    l,h,q=term;f=dict(full)
    f['left'],f['types']=l,h
    f['compatible']=(q>0)&((l>0).any(1)&(h>0).any(1))[:,None]
    groups=((l>0)[:,:,None]&(h>0)[:,None,:]).reshape(len(l),-1).astype(np.int64)
    f['new_const']=(groups.T@(f['compatible'].astype(np.int64)@f['const_value']))>0
    if index==3:
        # Difference of contractions becomes ONE signed contraction. Keep the
        # original K normalization even though the feature width is now 2K.
        f['x']=np.concatenate([full['x'],full['old_x']],axis=1)
        f['y']=np.concatenate([full['y'],-full['old_y']],axis=1)
    return matrix_packet(f),f


def evolving_fixture():
    base=fixture();cases=[('multi_value_missing_port',copy.deepcopy(base))]
    base['contexts'].append([14,3]);cases.append(('late_context',copy.deepcopy(base)))
    base['contexts'].append([11,3]);cases.append(('additional_context',copy.deepcopy(base)))
    # Canonicalized source rows, as would be supplied by native rebuild.
    base['contexts']=sorted({(e,3 if c==2 else c) for e,c in base['contexts']})
    base['constants']=[[*row[:3],3 if row[3]==2 else row[3]] for row in base['constants']]
    cases.append(('union_context',copy.deepcopy(base)))
    base['contexts']=[r for r in base['contexts'] if tuple(r)!=(11,3)]
    cases.append(('removed_context',copy.deepcopy(base)))
    return cases


def main():
    p=argparse.ArgumentParser();p.add_argument('--snapshot',type=Path,required=True)
    p.add_argument('--native',type=Path,required=True);p.add_argument('--gpu',type=Path)
    p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    a.output.mkdir(parents=True,exist_ok=False)
    loop,branch,cmp,provenance=load_snapshot(a.snapshot)
    result={'scope':'manual projected-binding representation on real nonrecursive Pass-3 queries; not full runtime or speed benchmark',
            'snapshot':provenance,'native_sha256':hashlib.sha256(a.native.read_bytes()).hexdigest(),
            'loop':{},'synthetic_updates':{},'gpu':None}
    packets=[];expected=[]
    for name,data in [*evolving_fixture(),('unstructured_64_keys',unstructured_fixture()),('raytrace',loop)]:
        report,f,answer=loop_check(data)
        report['native']=check_loop(a.native,data,answer,a.output/name)
        if name=='raytrace':result['loop']=report
        else:result['synthetic_updates'][name]=report
        packets.append(matrix_packet(f));expected.append(f)
    result['delta']={}
    for name,data,fresh in [
        ('all_old',fixture(),{}),
        ('raytrace_synthetic_watermark',loop,{k:{i for i in range(len(loop[k])) if i%7==0}
                                             for k in ('loops','contexts','types','constants')})]:
        full,terms=delta_terms(data,fresh);answer=delta_effects(data,full,terms)
        assert answer==delta_reference(data,fresh)
        record=check_loop(a.native,data,answer,a.output/name,fresh)
        record['scope']='explicit fresh-row predicate on current live facts; not a captured production watermark'
        record['factor_terms']=len(terms);record['expands_complete_binding_state']=False
        result['delta'][name]=record
        for i,term in enumerate(terms):
            packet,f=term_packet(full,term,i);packets.append(packet);expected.append(f)
    found,stats,_=if_projection(branch)
    assert found==if_reference(branch)
    result['if']={**stats,'projected_effects':len(found),'exact_projected_reference':True,
                  'native':check_if(a.native,branch,found,a.output/'raytrace-if'),
                  'gpu_tested':False,'warning':'joint signature is sparse; not a dense-GEMM success'}
    masked,masked_stats=if_masked_projection(branch);assert masked==found
    result['if']['masked']=masked_stats
    small=crossed_if_fixture();result['if']['native_updates']=[]
    for stage in range(3):
        if stage==1:small['args'].append([24,1,4]);small['gets'].append([24,4,71])
        if stage==2:small['gets']=[[p,i,70 if v==71 else v] for p,i,v in small['gets']]
        answer,stats=if_masked_projection(small);assert answer==if_reference(small)
        result['if']['native_updates'].append({**stats,**check_if(a.native,small,answer,a.output/f'if-stage-{stage}')})
    combined,separate,stats=comparator_projection(cmp)
    reference=comparator_reference(cmp)
    assert separate==reference and combined==set().union(*reference.values())
    result['smaller']={**stats,'smaller_rows':len(cmp['smaller']),'projected_union_pairs':len(combined),
                       'native':check_comparator(a.native,cmp,separate,a.output/'raytrace-comparators'),
                       'exact_projected_reference':True,'gpu_tested':False,
                       'warning':'bounds eliminate guards but are an elementwise/reduction path, not a GEMM success'}
    fused,fused_stats=fused_comparator_matrix(cmp);assert fused==combined
    result['smaller']['fused_matrix']=fused_stats
    if a.gpu:
        for f in expected:
            k=f['x'].shape[1]
            assert k*int(np.max(np.abs(f['x']),initial=0))*int(np.max(np.abs(f['y']),initial=0))<2**24
            assert f['compatible'].size<2**24
        inp=a.output/'gpu-input.json';inp.write_text(json.dumps({'jobs':packets}))
        out=a.output/'gpu-output.json';subprocess.run([str(a.gpu.resolve()),str(inp),str(out)],check=True)
        gpu=json.loads(out.read_text())
        for g,f in zip(gpu['jobs'],expected,strict=True):
            np.testing.assert_array_equal(np.array(g['compatible']).reshape(f['compatible'].shape),f['compatible'])
            np.testing.assert_array_equal(np.array(g['new_const']).reshape(f['new_const'].shape),f['new_const'])
        result['gpu']={k:v for k,v in gpu.items() if k!='jobs'}
        result['gpu'].update({'all_jobs_exact':True,'jobs':len(packets),'worker_sha256':hashlib.sha256(a.gpu.read_bytes()).hexdigest()})
    (a.output/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__=='__main__':main()

"""Supplement the frozen scale dataset with PRE-terms correlated cost tuples."""
import argparse,gzip,json,time
from pathlib import Path
import numpy as np
from affine_terms import compile_program,contexts,pair_index
from scale_state import DeltaState,answers,changes
from study import Oracle,sha


def capture(dataset,workload,frontend,out):
    d=json.loads((dataset/'dataset.json').read_text());w=json.loads(workload.read_text())
    assert sha(workload)==d['workload_sha256']
    assert sha(frontend)==d['frontend_sha256'] and sha(Path(d['source']))==d['source_sha256']
    out.mkdir(parents=True,exist_ok=False);native=Oracle(frontend,Path(d['source']),out)
    program=compile_program(native.core);tables=sorted({r['anchor'] for r in program['recipes']}|{'TCPair','Smaller','ExtractedExpr'})
    state=DeltaState();at=0;records=[];started=time.perf_counter();native_seconds=0
    journal=gzip.open(out/'observations.jsonl.gz','wt')
    def observe(op):
        p=native.ask(op);journal.write(json.dumps({'at':at,'packet':p},separators=(',',':'))+'\n');state.apply(p)
    def advance(end):
        nonlocal at,native_seconds
        if end==at:return
        expected=w['native_trace'][at:end];reply=native.ask({'op':'time_runs','sets':[x['set'] for x in expected]})
        assert [x['updated'] for x in reply['steps']]==[x['updated'] for x in expected],(at,end)
        native_seconds+=reply['seconds'];at=end
    try:
        observe({'op':'observe_tables','tables':tables})
        for f in d['frames']:
            if f['stage']!='terms':continue
            advance(f['start']);observe({'op':'observe_delta'});pre=state.snapshot()
            z=np.load(dataset/f['file']);keys=[tuple(d['roots'][i]) for i in z['roots']]
            begin=time.perf_counter();params,meta,_,pairs=contexts(pre,keys,program);prep=time.perf_counter()-begin
            before=answers(pre,native.core,'ExtractedExpr')
            advance(f['end']);observe({'op':'observe_delta'});post=state.snapshot();after=answers(post,native.core,'ExtractedExpr')
            gain,loss,present,size,event=changes(keys,before,after,state.lineage)
            np.testing.assert_array_equal(gain,z['gain']);np.testing.assert_array_equal(loss,z['loss'])
            np.testing.assert_array_equal(size,z['before_count'])
            pp,_=pair_index(post);improve=[]
            for k in keys:
                a=[v for _,v in pairs.get(k,[])];b=[v for _,v in pp.get(state.lineage.find(k),[])]
                improve.append(bool(a and b and min(b)<min(a)))
            name=f"frame-{f['id']:03}.npz";np.savez_compressed(out/name,params=params,meta=meta,cost_improved=np.array(improve),gain=gain)
            record={'id':f['id'],'file':name,'sha256':sha(out/name),'rows':len(keys),'contexts':len(params),'alignment_seconds':prep,
                    'complete_contexts':int(meta[:,2].sum()),'cost_improved':sum(improve),'labels_exact':True}
            records.append(record);print(json.dumps(record),flush=True)
        advance(len(w['native_trace']))
    finally:journal.close();native.close()
    result={'schema':'affine-terms-capture/v1','dataset_sha256':sha(dataset/'dataset.json'),'workload_sha256':sha(workload),
            'source_sha256':d['source_sha256'],'frontend_sha256':sha(frontend),'program':program,'frames':records,
            'native_runs':at,'native_trace_exact':True,'native_seconds_diagnostic':native_seconds,'capture_seconds':time.perf_counter()-started,
            'journal_sha256':sha(out/'observations.jsonl.gz'),'scope':'offline PRE-stage typed contexts; CPU alignment is charged, not GPU-resident discovery'}
    (out/'capture.json').write_text(json.dumps(result,indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser()
    for n in ['dataset','workload','frontend','output']:p.add_argument('--'+n,type=Path,required=True)
    a=p.parse_args();capture(a.dataset,a.workload,a.frontend,a.output)

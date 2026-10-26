"""All real Saturate visits, lossless table deltas, typed output-change labels."""
import argparse
from collections import Counter
import json
from pathlib import Path
import time
import numpy as np
from features import STAGES, View, stage_spans
from residual import pack_context
from residual_capture import write_packet
from scale_state import DeltaState, selected_tables, answers, changes, holdout_group
from study import Oracle, sha


def capture(workload,frontend,out):
    out.mkdir(parents=True,exist_ok=False)
    saved=json.loads(workload.read_text());source=Path(saved['source'])
    if sha(source)!=saved['source_sha256']:raise ValueError('source changed')
    spans,total=stage_spans(saved['core']['schedule'],saved['native_trace'],first_per_cycle=False)
    native=Oracle(frontend,source,out)
    if native.core['schedule']!=saved['core']['schedule']:raise ValueError('schedule changed')
    tables,programs=selected_tables(native.core);view=View(native.core);state=DeltaState()
    state.apply(native.ask({'op':'observe_tables','tables':tables}))
    vocabulary=[];token_ids={};signatures=[];signature_ids={};roots=[];rootids={};records=[];at=0
    packs={s:[] for s in programs};last='initial';deltas=0
    def intern(tokens):
        key=tuple(sorted(tokens,key=repr))
        if key not in signature_ids:
            ids=[]
            for t in key:
                if t not in token_ids:token_ids[t]=len(vocabulary);vocabulary.append(t)
                ids.append(token_ids[t])
            signature_ids[key]=len(signatures);signatures.append(ids)
        return signature_ids[key]
    def rootid(key):
        if key not in rootids:rootids[key]=len(roots);roots.append(key)
        return rootids[key]
    def advance(end):
        nonlocal at
        if end<at:raise ValueError('overlapping sampled saturations')
        if end==at:return
        trace=saved['native_trace'][at:end];r=native.ask({'op':'time_runs','sets':[x['set'] for x in trace]})
        if [x['updated'] for x in r['steps']]!=[x['updated'] for x in trace]:raise AssertionError(f'native trace changed at {at}')
        at=end
    try:
        for index,span in enumerate(spans):
            advance(span['start']);start=time.perf_counter();p=native.ask({'op':'observe_delta'});state.apply(p)
            pre=state.snapshot();observation=time.perf_counter()-start;start=time.perf_counter()
            samples,stats=view.extract(pre,span['stage'],last,prestate=True);last=span['stage'];keys=[x[0] for x in samples]
            before=answers(pre,native.core,STAGES[span['stage']][1])
            ids=np.array([[intern(x[1]),intern(x[3]),intern(x[4])] for x in samples],np.uint32)
            root=np.array([rootid(k) for k in keys],np.uint32)
            supported=span['stage'] in programs
            if supported:
                folder=out/span['stage'];folder.mkdir(exist_ok=True)
                c=pack_context(native.core,pre,keys,programs[span['stage']]);file=f'frame-{index:03}.bin';write_packet(folder/file,c)
                packs[span['stage']].append({'cycle':index,'outer_cycle':span['cycle'],'packet':file,'rows':len(keys)})
            prep=time.perf_counter()-start
            advance(span['end']);start=time.perf_counter();q=native.ask({'op':'observe_delta'});state.apply(q);post=state.snapshot()
            after=answers(post,native.core,STAGES[span['stage']][1]);gain,loss,present,sizes,event=changes(keys,before,after,state.lineage)
            observation+=time.perf_counter()-start
            target=next(t for t in native.core['tables'] if t['name']==STAGES[span['stage']][1])
            unary_relation=len(target['schema'])==2 and target['schema'][-1]['name'].startswith('___')
            eligible=np.array([n==0 or not unary_relation for n in sizes],bool)
            name=f'frame-{index:03}.npz'
            np.savez_compressed(out/name,features=ids,roots=root,gain=np.array(gain,bool),loss=np.array(loss,bool),
                                present=np.array(present,bool),before_count=np.array(sizes,np.uint32),eligible=eligible,
                                event=np.frombuffer(b''.join(event),dtype='<u8'))
            oldkeys={state.lineage.find(k) for k in keys}
            outside=sum(bool(v) for k,v in after.items() if k not in oldkeys)
            r=dict(span,id=index,file=name,file_sha256=sha(out/name),rows=len(keys),eligible=int(eligible.sum()),
                   gain=int(sum(gain)),loss=int(sum(loss)),new_presence=sum(g and n==0 for g,n in zip(gain,sizes)),
                   changed_existing=sum(g and n>0 for g,n in zip(gain,sizes)),outside_pre_domain=outside,
                   projection_supported=supported,observation_seconds=observation,feature_seconds=prep,
                   inserts=len(p['insert'])+len(q['insert']),removals=len(p['remove'])+len(q['remove']),
                   union_edges=len(p['aliases'])+len(q['aliases']))
            records.append(r)
            print(json.dumps({k:r[k] for k in ['id','stage','cycle','rows','gain','changed_existing','outside_pre_domain']}),flush=True)
            (out/'progress.json').write_text(json.dumps({'frames':len(records),'total':len(spans),'native_runs':at,'bytes':state.bytes}))
            del pre,post,samples
        advance(len(saved['native_trace']));state.apply(native.ask({'op':'observe_delta'}))
        canonical=[state.lineage.find(k) for k in roots];group_ids={k:i for i,k in enumerate(sorted(set(canonical)))}
        groups=np.array([group_ids[k] for k in canonical],np.uint32);held=np.array([holdout_group(k) for k in canonical],bool)
        np.savez_compressed(out/'lineage.npz',groups=groups,heldout=held)
        for stage,frames in packs.items():
            (out/stage/'capture.json').write_text(json.dumps({'program':programs[stage],'frames':frames},separators=(',',':')))
        result={'schema':'polyspine-scale/v1','source':str(source),'source_sha256':sha(source),'frontend_sha256':sha(frontend),
                'workload_sha256':sha(workload),'native_runs':at,'native_trace_exact':True,'frames':records,
                'vocabulary':vocabulary,'signatures':signatures,'roots':roots,'lineages':len(group_ids),
                'heldout_lineages':len({int(groups[i]) for i in range(len(roots)) if held[i]}),
                'programs':programs,'observed_tables':tables,'wire_bytes':state.bytes,'all_stage_invocations':total,
                'lineage_sha256':sha(out/'lineage.npz'),
                'capture_sources':{p.name:sha(p) for p in [Path(__file__),Path(__file__).with_name('scale_state.py'),Path(__file__).with_name('features.py')]},
                'scope':'all 182 actual saturation invocations of four selected families in saved native Pass 3; projected output gains/updates, not full rule bindings',
                'target_semantics':'new observable answer payload after saturation, modulo actual union; function outputs include one-level constructor payload (including TCPair cost)',
                'selection':'all selected stage visits, every pre-existing Expr root, no positive/size-based sampling'}
        (out/'dataset.json').write_text(json.dumps(result,separators=(',',':')))
        return result
    except Exception as e:
        (out/'failure.json').write_text(json.dumps({'error':str(e),'native_runs':at,'frames':records},indent=2));raise
    finally:native.close()


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--workload',required=True,type=Path);p.add_argument('--frontend',required=True,type=Path);p.add_argument('--output',required=True,type=Path)
    a=p.parse_args();capture(a.workload,a.frontend,a.output)

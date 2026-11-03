"""Supplement missing pre-stage state; reuse old labels and complete native trace."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys
import time
import numpy as np
from features import View, labels
from residual import compile_recipes, pack_context, project, frozen_closure
from study import Oracle, sha


def write_packet(path, c):
    with path.open('wb') as f:
        f.write(b'POLYRES1')
        f.write(struct.pack('<8I',c['state_width'],c['predicates'],len(c['programs']),c['tasks'],
                            len(c['rows']),len(c['code']),len(c['queries']),8))
        for name,dtype in [('seed','<f4'),('programs','<u4'),('rows','<u4'),('code','<u4'),('queries','<u4')]:
            f.write(c[name].astype(dtype).tobytes())


def capture(dataset, frontend, out):
    out.mkdir(parents=True,exist_ok=False)
    old=json.loads((dataset/'dataset.json').read_text());workload=json.loads(Path(old['workload']).read_text())
    source=Path(old['source'])
    if sha(frontend)!=old['frontend_sha256'] or sha(source)!=old['source_sha256']:
        raise ValueError('native source or oracle fingerprint changed')
    native=Oracle(frontend,source,out);view=View(native.core);program=compile_recipes(native.core)
    (out/'program.json').write_text(json.dumps(program,indent=2)+'\n')
    at=0;frames=[]
    def advance(end):
        nonlocal at
        if end==at:return
        expected=workload['native_trace'][at:end]
        result=native.ask({'op':'time_runs','sets':[x['set'] for x in expected]})
        if [s['updated'] for s in result['steps']] != [s['updated'] for s in expected]:
            raise AssertionError(f'native trace diverged at {at}')
        at=end
    try:
        for frame in old['frames']:
            if frame['stage']!='always-run':continue
            advance(frame['start']);start=time.perf_counter();snapshot=native.ask({'op':'snapshot'})
            observation=time.perf_counter()-start;start=time.perf_counter()
            rows,_=view.extract(snapshot,'always-run','type-analysis')
            old_array=np.load(dataset/frame['file']);ids=old_array['signatures'][:,3]
            if len(rows)!=len(ids):raise AssertionError('prediction domain changed')
            # Compare entire structural/Polyspine bags and entry visibility in
            # order before borrowing any old labels. No new post-stage export.
            normalize=lambda x:json.dumps(x,separators=(',',':'))
            for row,sig,known in zip(rows,ids,old_array['before']):
                bags=[[old['vocabulary'][t] for t in bag] for bag in old['signatures'][int(sig)]]
                if normalize([row[1],row[4]])!=normalize(bags) or row[5]!=bool(known):
                    raise AssertionError('pre-stage identity/order/features changed')
            keys=[r[0] for r in rows];context=pack_context(native.core,snapshot,keys,program)
            packing=time.perf_counter()-start;stem=f"cycle-{frame['cycle']:02}"
            write_packet(out/f'{stem}.bin',context)
            # The independent oracle is diagnostic only; GPU receives raw rows,
            # seeds and column programs, never this expanded incidence list.
            start=time.perf_counter();projection=project(context);closed,clauses=frozen_closure(context)
            diagnostic=time.perf_counter()-start
            # A feature-equivalent row order alone does not establish the old
            # labels' identity. Check entry/exit labels in this very replay too.
            native.ask({'op':'track_anchors'})
            advance(frame['end']);started=time.perf_counter();post=native.ask({'op':'snapshot'})
            observation+=time.perf_counter()-started
            np.testing.assert_array_equal(np.array(labels(post,keys,'ExprIsPure'),np.uint8),old_array['y'])
            del post
            np.savez_compressed(out/f'{stem}.npz',y=old_array['y'],before=old_array['before'],
                                old_signatures=old_array['signatures'],projection=projection,frozen=closed,
                                keys=np.array([str(k) for k in keys]))
            # Preserve a small exact replayable carrier for further feature work,
            # not another full egraph snapshot or gigantic match history.
            np.savez_compressed(out/f'{stem}-context.npz',**{k:context[k] for k in ['seed','programs','rows','code','queries']})
            pending=old_array['before']==0;y=old_array['y'].astype(bool)
            rec={'cycle':frame['cycle'],'old_frame':frame['file'],'start':frame['start'],
                 'packet':f'{stem}.bin','data':f'{stem}.npz','packet_sha256':sha(out/f'{stem}.bin'),
                 'data_sha256':sha(out/f'{stem}.npz'),'observation_seconds':observation,'packing_seconds':packing,
                 'diagnostic_seconds':diagnostic,'raw_rows':context['raw_anchor_rows'],'coordinates':len(context['seed']),
                 'rule_rows':context['tasks'],'clauses':len(clauses),'incidences':sum(len(d) for _,d in clauses),
                 'query_rows':len(keys),'pending_positive':int((pending&y).sum()),
                 'frozen_true_positive':int((pending&y&closed).sum()),
                 'frozen_false_positive':int((pending&~y&closed).sum()),
                 'eight_layer_true_positive':int((pending&y&(projection[:,-1]>0)).sum())}
            rec['native_exit_labels_checked']=True
            frames.append(rec);print(json.dumps(rec),flush=True)
            (out/'progress.json').write_text(json.dumps({'frames':len(frames),'native_runs':at}))
            del snapshot,rows,context
        advance(len(workload['native_trace']))
        report={'schema':'polyspine-residual-capture/v1','old_dataset':str(dataset.resolve()),
                'old_dataset_sha256':sha(dataset/'dataset.json'),'source_sha256':old['source_sha256'],
                'frontend_sha256':old['frontend_sha256'],'native_runs':at,'trace_exact':True,
                'program':program,'frames':frames,
                'sources':{p.name:sha(p) for p in [Path(__file__),Path(__file__).with_name('residual.py')]},
                'boundary':'only pre-stage raw rows, aliases and committed unary seeds; old post-stage labels reused after exact feature/order checks',
                'limits':['Frozen pre-stage support program, not full always-run semantics.',
                          'No new structural row, future union, function update or unsupported guard is predicted as fact.',
                          'Rules with multiple structural atoms/guards remain explicit unsupported paths.']}
        (out/'capture.json').write_text(json.dumps(report,indent=2)+'\n')
    finally:native.close()


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--dataset',type=Path,required=True);p.add_argument('--frontend',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();capture(a.dataset,a.frontend,a.output)

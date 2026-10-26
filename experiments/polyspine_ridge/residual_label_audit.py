"""Independently confirm reused labels at the persisted typed pre-stage keys."""
import argparse
import ast
import json
from pathlib import Path
import time
import numpy as np
from features import labels
from study import Oracle, sha


def audit(capture_dir,frontend,out):
    out.mkdir(parents=True,exist_ok=False);start=time.perf_counter()
    c=json.loads((capture_dir/'capture.json').read_text());old=json.loads((Path(c['old_dataset'])/'dataset.json').read_text())
    w=json.loads(Path(old['workload']).read_text());source=Path(old['source'])
    if sha(frontend)!=c['frontend_sha256'] or sha(source)!=c['source_sha256']:raise ValueError('oracle mismatch')
    native=Oracle(frontend,source,out);at=0;checks=[]
    def advance(end):
        nonlocal at
        if end==at:return
        expected=w['native_trace'][at:end]
        got=native.ask({'op':'time_runs','sets':[x['set'] for x in expected]})
        if [x['updated'] for x in got['steps']] != [x['updated'] for x in expected]:raise AssertionError('native update mismatch')
        at=end
    try:
        for frame in c['frames']:
            span=next(x for x in old['frames'] if x['file']==frame['old_frame']);z=np.load(capture_dir/frame['data'])
            keys=[ast.literal_eval(str(k)) for k in z['keys']]
            advance(span['start']);truth=set(native.ask({'op':'unary_predicates','tables':['ExprIsPure']})['ExprIsPure'])
            np.testing.assert_array_equal(np.array([k[2] in truth for k in keys]),z['before'])
            native.ask({'op':'track_anchors'});advance(span['end']);post=native.ask({'op':'snapshot'})
            actual=np.array(labels(post,keys,'ExprIsPure'),np.uint8)
            np.testing.assert_array_equal(actual,z['y'])
            checks.append({'cycle':frame['cycle'],'entry_rows':len(keys),'entry_and_exit_labels_exact':True})
            print('validated labels',frame['cycle'],len(keys),flush=True)
        advance(len(w['native_trace']))
        (out/'audit.json').write_text(json.dumps({'scope':'independent native endpoint audit of borrowed labels and entry state',
            'source_sha256':c['source_sha256'],'capture_sha256':sha(capture_dir/'capture.json'),
            'native_runs':at,'checks':checks,'wall_seconds_diagnostic':time.perf_counter()-start},indent=2)+'\n')
    finally:native.close()


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--capture',type=Path,required=True);p.add_argument('--frontend',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();audit(a.capture,a.frontend,a.output)

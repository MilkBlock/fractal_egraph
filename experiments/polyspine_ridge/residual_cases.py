"""Native diagnosis of remaining frozen-carrier misses; never model features."""
import argparse
import ast
import json
from pathlib import Path
import time
import numpy as np
from features import identity
from study import Oracle, sha


def inspect(capture_dir, frontend, out):
    out.mkdir(parents=True,exist_ok=False)
    cap=json.loads((capture_dir/'capture.json').read_text());old=json.loads((Path(cap['old_dataset'])/'dataset.json').read_text())
    w=json.loads(Path(old['workload']).read_text());source=Path(old['source'])
    if sha(source)!=cap['source_sha256'] or sha(frontend)!=cap['frontend_sha256']:raise ValueError('oracle changed')
    wanted=[]
    for f in cap['frames']:
        if f['cycle']<6:continue
        z=np.load(capture_dir/f['data']);take=(z['before']==0)&(z['y']==1)&(~z['frozen'])
        if take.any():wanted.append((f,[ast.literal_eval(str(k)) for k in z['keys'][take]]))
    native=Oracle(frontend,source,out);at=0;cases=[]
    def advance(end):
        nonlocal at
        if end==at:return
        expected=w['native_trace'][at:end]
        got=native.ask({'op':'time_runs','sets':[x['set'] for x in expected]})
        if [x['updated'] for x in got['steps']] != [x['updated'] for x in expected]:raise AssertionError('trace mismatch')
        at=end
    def visible(snapshot):return [r for r in snapshot['rows'] if not r['subsumed']]
    def neighborhood(rows,key):
        roots=[r for r in rows if identity(r['values'][-1])==key and not r['table'].startswith('___')]
        args={identity(v) for r in roots for v in r['values'][:-1] if v['sort']=='Expr'}
        interfaces=[r for r in rows if r['table'] in ['tuple-length','ExprIsPure','HasType','HasArgType']
                    and identity(r['values'][0]) in args|{key}]
        return {'constructors':roots,'argument_interfaces':interfaces}
    try:
        for f,keys in wanted:
            span=next(x for x in old['frames'] if x['file']==f['old_frame'])
            advance(span['start']);before=native.ask({'op':'snapshot'});native.ask({'op':'track_anchors'})
            advance(span['end']);after=native.ask({'op':'snapshot'})
            aliases={(a['sort'],a['old']):a['now'] for a in after['anchors']}
            pre=visible(before);post=visible(after)
            for key in keys:
                new=(key[0],'eq',aliases.get((key[0],key[2]),key[2]))
                a=neighborhood(pre,key);b=neighborhood(post,new)
                cases.append({'cycle':f['cycle'],'before_key':key,'after_key':new,'before':a,'after':b,
                              'new_constructor_heads':sorted({r['table'] for r in b['constructors']}-{r['table'] for r in a['constructors']})})
        (out/'cases.json').write_text(json.dumps({'scope':'post-hoc native diagnosis, excluded from training/features',
            'prefix_runs_checked':at,'source_sha256':cap['source_sha256'],'cases':cases},indent=2)+'\n')
        print([(c['before_key'],c['new_constructor_heads']) for c in cases])
    finally:native.close()


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--capture',type=Path,required=True);p.add_argument('--frontend',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();inspect(a.capture,a.frontend,a.output)

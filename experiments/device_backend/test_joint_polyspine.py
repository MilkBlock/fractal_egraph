"""Complete three-ruleset replay, adversarial prediction and native checks."""
import argparse,json,struct,subprocess
from pathlib import Path
from run import run,read
from compare_runs import compare
from compiler import compile_program
HERE=Path(__file__).resolve().parent
BASE=(HERE/'fixtures/polyspine-terms.egg').read_text()
SCHEDULE='(run-schedule (saturate (run terms) (saturate (run terms-helpers) (saturate (run terms-helpers-helpers)))))\n'

def rejected_transaction(build,file,out,reason,capacity=4096,stage_contexts=False,block_merge=False):
    out.mkdir(parents=True)
    with (out/'native.log').open('w') as log:
        native=subprocess.Popen([str((build/'frontend').resolve()),str(file.resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log,text=True)
        core=json.loads(native.stdout.readline());native.stdin.write('{"op":"quit"}\n');native.stdin.flush();native.wait();native.stdin.close();native.stdout.close()
    p=compile_program(core,capacity=capacity,joint_polyspine=True,stage_contexts=stage_contexts,block_merge=block_merge,block_merge_audit=block_merge);p['stage_context_audit']=stage_contexts;assert p['joint_polyspine']['program']
    path=out/'program.json';path.write_text(json.dumps(p))
    with (out/'gpu.log').open('w') as log:
        gpu=subprocess.Popen([str((build/'worker').resolve()),str(path.resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log)
        try:
            assert read(gpu.stdout,4)==b'DVB7'
            def command(op):gpu.stdin.write(struct.pack('<II',op,0));gpu.stdin.flush()
            def snapshot():
                command(2);raw=read(gpu.stdout,36);m=struct.unpack('<9I',raw)
                raw+=read(gpu.stdout,m[0]*p['stride']*8+m[1]*4+m[8]*4)
                n=read(gpu.stdout,4);raw+=n+read(gpu.stdout,struct.unpack('<I',n)[0]*8);return raw
            before=snapshot();command(6);read(gpu.stdout,100)
            header=struct.unpack('<8I',read(gpu.stdout,32));read(gpu.stdout,8);stats=struct.unpack('<128q',read(gpu.stdout,1024));read(gpu.stdout,header[2]*28)
            assert header[0]==0 and header[1]==reason,header
            assert all(x==0 for x in stats[64:80]),'numeric RHS payload leaked into control telemetry'
            assert snapshot()==before,'rejected macro changed published rows, IDs, parents or metadata'
            command(0);gpu.wait(timeout=30)
        finally:
            if gpu.poll() is None:gpu.terminate();gpu.wait()
            gpu.stdin.close();gpu.stdout.close()
    return {'accepted':False,'reason':reason,'published_state_byte_identical':True,'numeric_payload_not_downloaded':True}

def tests(build,out,refresh_audit=False):
    out.mkdir(parents=True,exist_ok=False);results={}
    def pair(name,source,accepted=True,**options):
        training=options.pop('polyspine_training',False)
        file=out/(name+'.egg');file.write_text(source)
        before,after=out/(name+'-normal'),out/(name+'-joint')
        a=run(file,build,before,matrix_saturate=True,**options)
        b=run(file,build,after,matrix_saturate=True,joint_polyspine=True,polyspine_training=training or refresh_audit,polyspine_refresh_audit=refresh_audit,**options)
        assert any(c['accepted'] for c in b['joint_polyspine']['calls'])==accepted,name
        result=compare(before,after,device_states=True,semantic_device=True,saturation_macros=True)
        result['native_checks']=[a['checks'][-1],b['checks'][-1]];result['calls']=b['joint_polyspine']['calls'];results[name]=result
        print(name,[(c['accepted'],c['marker_bindings'],c['private_phases']) for c in b['joint_polyspine']['calls']],flush=True)
        return b
    tree=BASE+'''(let a (Num 1)) (let b (Num 2)) (let ab (Add a b)) (let t (Triple ab b a))
'''+SCHEDULE+'(check (= (ExtractedExpr t) (TCPair (TermTriple (TermAdd (TermNum 1) (TermNum 2)) (TermNum 2) (TermNum 1)) 6)))'
    b=pair('multi-stage-tree',tree)
    call=b['joint_polyspine']['calls'][0];assert call['global_publications']==1 and call['statistics'][87]>=4 and call['statistics'][59]>0
    source=BASE+'''(let a (Num 1)) (let b (Num 2)) (let t (Add a b))
(set (ExtractedExpr t) (TCPair (TermNum 99) 1))
'''+SCHEDULE+'(check (PotentialExtractedExpr t (TCPair (TermAdd (TermNum 1) (TermNum 2)) 3)))'
    pair('non-winning-candidate-retained',source)
    source=BASE+'''(let a (Num 1)) (let b (Num 2)) (let c (Num 3)) (let d (Num 4)) (let e (Num 5))
(let left (Add a b)) (let right (Add (Add c d) e)) (union left right)
'''+SCHEDULE
    pair('one-root-multiple-spines',source)
    source=BASE+'''(let a (External 1)) (let b (External 2))
(let p (TCPair (TermNum 11) 1)) (union p (TCPair (TermNum 12) 5))
(set (ExtractedExpr a) p) (set (ExtractedExpr b) (TCPair (TermNum 20) 2)) (let t (Add a b))
'''+SCHEDULE+'''(check (PotentialExtractedExpr t (TCPair (TermAdd (TermNum 11) (TermNum 20)) 4)))
(check (PotentialExtractedExpr t (TCPair (TermAdd (TermNum 12) (TermNum 20)) 8)))'''
    pair('complete-correlated-cost-witnesses',source)
    source=BASE+'''(let a (External 1)) (let p (TCPair (TermNum 11) 1)) (union p (TCPair (TermNum 12) 5))
(set (ExtractedExpr a) p) (let t (Add a a))
'''+SCHEDULE+'''(check (PotentialExtractedExpr t (TCPair (TermAdd (TermNum 11) (TermNum 12)) 7)))
(check (PotentialExtractedExpr t (TCPair (TermAdd (TermNum 12) (TermNum 11)) 7)))'''
    pair('aliased-ports-keep-multiplicity',source)
    source=BASE+'''(let a (Num 1)) (let b (Num 2)) (let t (Add a b))
(run terms 1) (run terms-helpers 1)
'''+SCHEDULE+'(check (PotentialExtractedExpr t (TCPair (TermAdd (NoTerm) (NoTerm)) 20000000000000001)))'
    pair('finite-run-sentinel-visible-at-entry',source)
    source=BASE+'''(let a (External 1)) (let b (Num 2)) (Add a b)
'''+SCHEDULE
    pair('unresolved-port-remains-pending',source)
    source=BASE+'''(let a (External 1)) (let h (Hole 0))
(set (ExtractedExpr a) (Smaller (TCPair (TermNum 11) 1) h)) (let t (Add a a))
(ruleset external) (rule ((= h (Hole 0))) ((union h (TCPair (TermNum 12) 4))) :ruleset external)
'''+SCHEDULE+'(run external 1)\n'+SCHEDULE+'(check (PotentialExtractedExpr t (TCPair (TermAdd (TermNum 11) (TermNum 11)) 3)))'
    pair('late-comparison-input-resumes-parent',source)
    source=BASE+'''(let a (External 1)) (let h (Hole 0)) (union h (Smaller h (TCPair (TermNum 1) 1)))
(set (ExtractedExpr a) h) (Add a a)
'''+SCHEDULE
    pair('unfounded-cycle-no-invented-result',source)
    source=BASE+'''(let a (Num 1)) (let b (Num 2)) (let t (Add a b))
(ruleset hide) (rule ((= t (Add x y))) ((subsume (Add x y))) :ruleset hide)
'''+SCHEDULE+'(run hide 1)\n'+SCHEDULE
    pair('retired-source-owner-refresh',source)
    source=BASE+'''(let a (Num 1)) (let b (Num 2)) (Add a b)
(ruleset outside) (rule ((= a (Num 1)) (= b (Num 2))) ((union a b)) :ruleset outside)
'''+SCHEDULE+'(run outside 1)\n'+SCHEDULE
    pair('external-root-union-refresh',source)
    source=BASE+'''(let a (External 1)) (let b (External 2))
(set (ExtractedExpr a) (TCPair (TermNum 1) 2048)) (set (ExtractedExpr b) (TCPair (TermNum 2) 1)) (Add a b)
'''+SCHEDULE
    b=pair('exact-affine-matrix-result',source);assert b['joint_polyspine']['calls'][0]['statistics'][54]>0
    source=BASE+'''(let a (External 1)) (let b (External 2))
(set (ExtractedExpr a) (TCPair (TermNum 1) 10000000000000000)) (set (ExtractedExpr b) (TCPair (TermNum 2) 1)) (let t (Add a b))
'''+SCHEDULE+'(check (PotentialExtractedExpr t (TCPair (TermAdd (TermNum 1) (TermNum 2)) 10000000000000002)))'
    b=pair('large-cost-exact-fallback',source);assert b['joint_polyspine']['calls'][0]['statistics'][55]>0
    source=BASE+'''(let a (External 1)) (let b (External 2))
(set (ExtractedExpr a) (TCPair (TermNum 1) 9223372036854775807)) (set (ExtractedExpr b) (TCPair (TermNum 2) -9223372036854775807)) (let t (Add a b))
'''+SCHEDULE+'(check (PotentialExtractedExpr t (TCPair (TermAdd (TermNum 1) (TermNum 2)) 1)))'
    pair('signed-cancellation-is-not-erased',source)
    source=BASE+'''(let a (External 1)) (let c (TCPair (TermNum 1) 1)) (set (ExtractedExpr a) c)
(subsume (TCPair (TermNum 1) 1)) (Add a a)
'''+SCHEDULE
    pair('hidden-cost-falls-back',source,accepted=False)
    source=BASE+'''(function ObserveCost (TermAndCost) i64 :merge (min old new))
(Num 1) (Num 2) (Add (Num 1) (Num 2))
'''+SCHEDULE
    pair('uncertified-merge-footprint-falls-back',source,accepted=False)
    pair('ordinary-runs-are-not-fused',BASE+'(Add (Num 1) (Num 2)) (run terms 1) (run terms-helpers 1)',accepted=False)
    pair('empty-program',BASE+SCHEDULE)
    pair('non-delta-query-order',tree,delta_queries=False)
    source=BASE+'''(let a (External 1)) (let x (TCPair (TermNum 1) 1)) (let y (TCPair (TermNum 2) 2))
(let sentinel (TCPair (NoTerm) 10000000000000000)) (let mid (Smaller sentinel x))
(Smaller mid y) (Smaller x y) (set (ExtractedExpr a) mid) (let late (Add a a))
'''+SCHEDULE
    pair('late-source-survives-compaction',source,polyspine_training=True)
    folder=out/'late-source-survives-compaction-joint';p=json.loads((folder/'program.json').read_text())['joint_polyspine']['program']
    b=(folder/'polyspine-training/scope-0-interfaces.bin').read_bytes();n,m,f,c=struct.unpack_from('<4I',b,8);at=24+4*n
    markers=struct.unpack_from('<'+'I'*(m*8),b,at);at+=m*32+n*64*4+n*4+(n+1)*4+m*4
    payload=struct.unpack_from('<'+'q'*(m*c),b,at);at+=m*c*8;tables=struct.unpack_from('<'+'I'*m,b,at)
    for i in range(m):
        marker=markers[i*8:i*8+8];r=p['recipes'][marker[1]];assert tables[i]==r['table'] and payload[i*c+r['root_column']]==marker[2]
        for j,port in enumerate(r['inputs']):assert payload[i*c+port['column']]==marker[4+j]
    results['interface-export-before-compaction']=True
    # Exact fallback must preserve every result even if a model proposes none
    # of the true transitions, or every impossible transition.
    for name in ['multi-stage-tree','one-root-multiple-spines','complete-correlated-cost-witnesses']:
        base=out/(name+'-joint');program=json.loads((base/'program.json').read_text())['joint_polyspine']['program']
        for label,bias in [('all-negative',[-1,-1,-1]),('all-positive',[1,1,1]),('threshold',[.5,.5,.5])]:
            model=out/f'{name}-{label}.json';model.write_text(json.dumps({'schema':'resident-polyspine-ridge/v1','layout_sha256':program['layout_sha256'],'weights':[0]*(136*8),'scales':[1,1,1],'bias':bias,'diagnostic':'adversarial safety model, not a trained performance model'}))
            path=out/f'{name}-{label}';r=run(out/(name+'.egg'),build,path,joint_polyspine=True,polyspine_model=model,matrix_saturate=True,polyspine_refresh_audit=refresh_audit)
            assert all(c['accepted'] for c in r['joint_polyspine']['calls'])
            check=compare(base,path,device_states=True,semantic_device=True,saturation_macros=True)
            if label=='all-negative':assert sum(r['joint_polyspine']['calls'][0]['statistics'][92:95])==0
            else:
                for call in r['joint_polyspine']['calls']:
                    assert call['statistics'][92:95]==[call['root_contexts']*call['private_phases']]*3,'ridge bias/threshold was not applied'
            results[name+'-'+label]=check
            print(name,label,'exact fallback passed',flush=True)
    resumed=run(out/'multi-stage-tree.egg',build,out/'resumed',resume=out/'multi-stage-tree-joint',joint_polyspine=True,matrix_saturate=True)
    assert resumed['complete_schedule'] and resumed['checks'][-1]['exact_rows_values_equivalence_visibility'];results['resume']=True
    failures={
        'overflow':(BASE+'''(let a (External 1)) (let b (External 2))
(set (ExtractedExpr a) (TCPair (TermNum 1) 9223372036854775807)) (set (ExtractedExpr b) (TCPair (TermNum 2) 1)) (Add a b)
'''+SCHEDULE,5),
        'panic':(BASE+'(PotentialExtractedExpr (External 1) (TCPair (TermNum 1) -1))\n'+SCHEDULE,8)}
    for name,(source,code) in failures.items():
        file=out/(name+'.egg');file.write_text(source)
        results[name+'-rollback']=rejected_transaction(build,file,out/(name+'-transaction'),code)
        for mode in [False,True]:
            try:run(file,build,out/f'{name}-{mode}',joint_polyspine=mode)
            except RuntimeError as e:assert f'error={code}' in str(e),str(e)
            else:raise AssertionError('failure was suppressed')
        print(name,'rollback and original error preserved',flush=True)
    model=json.loads((out/'multi-stage-tree-all-positive.json').read_text());model['layout_sha256']='wrong'
    invalid=out/'invalid-model.json';invalid.write_text(json.dumps(model))
    try:run(out/'multi-stage-tree.egg',build,out/'invalid-model',joint_polyspine=True,polyspine_model=invalid)
    except ValueError as e:assert 'layout' in str(e)
    else:raise AssertionError('stale model accepted')
    results['stale-model-rejected']=True
    if refresh_audit:
        reports=[json.loads(p.read_text()) for p in out.glob('*/report.json')]
        calls=[c for r in reports if r['joint_polyspine']['feature_refresh_audit'] for c in r['joint_polyspine']['calls'] if c['accepted']]
        assert calls and sum(c['statistics'][98] for c in calls)>0
        for c in calls:
            st=c['statistics'];assert st[97]+st[98]==st[96]==st[99]+st[100]==st[103]
            assert st[99]>=st[97] and st[101]>=st[97] and (st[102]==0 or st[102]>=st[99])
        path=out/'full-refresh';full=run(out/'multi-stage-tree.egg',build,path,joint_polyspine=True,polyspine_model=out/'multi-stage-tree-all-positive.json',matrix_saturate=True,polyspine_full_refresh=True,polyspine_refresh_audit=True)
        for c in full['joint_polyspine']['calls']:assert c['statistics'][97]==c['statistics'][96]==c['statistics'][99]
        results['full-refresh-ablation']=compare(out/'multi-stage-tree-all-positive',path,device_states=True,semantic_device=True,saturation_macros=True)
        results['cached-feature-score-audit']={'phases':sum(c['private_phases'] for c in calls),'row_audits':sum(c['statistics'][103] for c in calls),'base_reused':sum(c['statistics'][98] for c in calls),'passed':True}
    (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');return results

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--audit-refresh',action='store_true');p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();r=tests(a.build,a.output,a.audit_refresh);print('passed',len(r),'joint cases')

"""One clean same-binary local-saturation pair; streaming semantic-boundary audit."""
import argparse,collections,json,os,subprocess
from pathlib import Path
from eggfuse.engine.cell_runtime import load_pipeline_artifact,prepare,execute
from eggfuse.engine.cpu import compare_native
from eggfuse.ir.cell_runtime import build
from experiments.cell_runtime.presearch_bench import SETTINGS,ROOT
from experiments.cell_runtime.compositions_bench import mode,metrics
from experiments.cell_runtime.suite import timed,fingerprint

def audit(path,program,out):
    out.mkdir();binary=ROOT/'engine/cell-runtime/target/release/examples/project_semantics'
    logs=[(out/(str(i)+'.log')).open('w') for i in (0,1)];procs=[];count=0;summaries=[]
    try:
        for i,log in enumerate(logs):
            procs.append(subprocess.Popen([str(binary),str(path)],stdout=subprocess.PIPE,stderr=log,text=True,
                env={**os.environ,'CELL_CPU_LOCAL_SAT':str(i),'CELL_CPU_LOCAL_SAT_AUDIT':'1'}))
        while True:
            lines=[p.stdout.readline() for p in procs]
            if not any(lines):break
            assert all(lines),'semantic boundary count differs'
            a,b=map(json.loads,lines)
            if a['kind']=='summary':assert b['kind']=='summary';summaries=[a,b];continue
            assert {k:v for k,v in a.items() if k!='state'}=={k:v for k,v in b.items() if k!='state'}
            compare_native(program['source_core'],b['state'],a['state']);count+=1
        assert all(p.wait()==0 for p in procs),'semantic streamer failed'
    finally:
        for p in procs:
            if p.poll() is None:p.kill()
            p.wait()
        for f in logs:f.close()
    assert summaries
    return {'boundaries':count,'all_rows_values_equivalence_visibility':True,'source_outer_sat_and_finite_run_boundaries':True,
        'per_local_binding_original_lhs_checked':True,'streamer_sha256':fingerprint(binary),'summaries':summaries}

def profile_existing(receipt, out):
    """Diagnose frozen timed lanes without repeating their timing runs."""
    saved=json.loads(receipt.read_text());path=Path(saved['program'])
    engine=ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime'
    assert saved['binary_sha256']==fingerprint(engine)
    assert saved['program_sha256']==fingerprint(path)
    program=json.loads(path.read_text());profiles={};summary={}
    for name,lane in saved['lanes'].items():
        env={**lane['measurement']['configuration'],'CELL_CPU_REFUTATION_PROFILE':'1'}
        r=execute(program,out/name,engine,environment=env)
        assert json.loads((out/name/'runtime/state.json').read_text())==json.loads((receipt.parent/name/'state.json').read_text())
        assert lane['all_candidate_visits']==r['stats']['direct_candidates']+r['stats']['residual_candidates']+r['multi_stage']['stats']['merge_candidate_rows']+r['stage_spine']['stats']['pivot_candidates']
        profiles[name]=r['refutation_depth'];rules=profiles[name]['rules']
        totals={key:collections.Counter() for key in ('reads','candidate_rows','refuted_before_depth','deepest_seed')}
        for rule in rules.values():
            assert sum(rule['deepest_seed'].values())==rule['seeds']
            for key in totals:totals[key].update({int(k):v for k,v in rule[key].items()})
        seeds=sum(rule['seeds'] for rule in rules.values());hist=totals['deepest_seed']
        summary[name]={'seeds':seeds,**totals,'probability_max_depth_at_least':{d:sum(v for k,v in hist.items() if k>=d)/seeds for d in range(1,max(hist,default=0)+1)}}
    base=profiles['ordinary']['rules']
    common={rid for rid,r in base.items() if r['seeds'] and all(p['rules'].get(rid,{}).get('seeds',0) for p in profiles.values())}
    mass=sum(base[rid]['seeds'] for rid in common)
    max_depth=max((int(d) for p in profiles.values() for r in p['rules'].values() for d in r['deepest_seed']),default=0)
    fixed={name:{d:sum(base[rid]['seeds']*sum(v for k,v in p['rules'][rid]['deepest_seed'].items() if int(k)>=d)/p['rules'][rid]['seeds'] for rid in common)/mass for d in range(1,max_depth+1)} for name,p in profiles.items()} if mass else {}
    result={'timing_eligible':False,'timed_lanes_reused_from':str(receipt.resolve()),'program_sha256':fingerprint(path),'binary_sha256':fingerprint(engine),
        'scope':'remaining ordinary matched delta seeds and inner table reads only; Spine entries, direct rules and singleton point projections outside these depth curves',
        'profiles':profiles,'summary':summary,'fixed_rule_weight_curves':fixed,'fixed_rule_weight_common_support_mass':mass/sum(r['seeds'] for r in base.values()),
        'fixed_rule_weight_scope':'original matched-seed rule weights, restricted to rules still observed in every lane; candidate identity changes; moved queries are not refutations',
        'all_diagnostic_endpoints_exact':True}
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    print('profiled', {n:{'seeds':v['seeds'],'inner_candidate_rows':sum(v['candidate_rows'].values())} for n,v in summary.items()},flush=True)

def main():
    p=argparse.ArgumentParser();g=p.add_mutually_exclusive_group(required=True)
    g.add_argument('--profile-existing',type=Path);g.add_argument('--compiled',type=Path);g.add_argument('--demo-chain',type=int);g.add_argument('--demo-stages',type=int)
    p.add_argument('--instances',type=int,default=128);p.add_argument('--frontend',type=Path)
    p.add_argument('--feature',choices=['local-sat','retained','spine-prefix','stage-spine','stage-partial','stage-shared'],default='local-sat')
    p.add_argument('--stage-ablation',action='store_true',help='stage-partial/stage-shared: ordinary, whole-query, partial and shared-entry lanes')
    p.add_argument('--prefix-ablation',action='store_true',help='spine-prefix only: ordinary, shared reads, shared prefixes')
    p.add_argument('--shared-ablation',action='store_true',help='retained only: ordinary, per-rule, shared-domain lanes')
    p.add_argument('--context-sets',default='always-run,type-analysis')
    p.add_argument('--output',type=Path,required=True);p.add_argument('--audit',action='store_true');a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    if a.profile_existing:return profile_existing(a.profile_existing.resolve(),out)
    if a.compiled:
        path,program,provenance=load_pipeline_artifact(a.compiled.resolve());scope='authenticated source_sk_mk pipeline'
    elif a.demo_stages:
        assert a.demo_stages>=2 and a.instances>0
        n=a.demo_stages
        declarations='(datatype E (N i64) (Seed E) (Known E) '+' '.join(f'(S{i} E)' for i in range(n))+')\n'+''.join(f'(ruleset s{i})\n' for i in range(n))
        rules='(rule ((Seed x)) ((Known x) (S0 x)) :ruleset s0)\n'+''.join(f'(rule ((S{i-1} x) (Known x)) ((S{i} x)) :ruleset s{i})\n' for i in range(1,n))
        source=declarations+rules+''.join(f'(Seed (N {i}))\n' for i in range(a.instances))+'(run-schedule (seq '+ ' '.join(f'(run s{i})' for i in range(n))+'))\n'
        src=out/'source.egg';src.write_text(source);program=build(prepare(src,a.frontend));path=out/'program.json';path.write_text(json.dumps(program))
        provenance={'source_sha256':fingerprint(src)};scope='synthetic source-scheduled multi-stage chain through native typed frontend; not eggcc performance'
    else:
        assert a.demo_chain>=2 and a.instances>0
        declarations='(datatype E (N i64) (Seed E) (Known E) '+ ' '.join(f'(S{i} E)' for i in range(a.demo_chain))+') (ruleset s)\n'
        rules='(rule ((Seed x)) ((Known x) (S0 x)) :ruleset s)\n'+''.join(f'(rule ((S{i} x) (Known x)) ((S{i+1} x)) :ruleset s)\n' for i in range(a.demo_chain-1))
        source=declarations+rules+''.join(f'(Seed (N {i}))\n' for i in range(a.instances))+'(run-schedule (saturate s))\n'
        src=out/'source.egg';src.write_text(source);program=build(prepare(src,a.frontend));path=out/'program.json';path.write_text(json.dumps(program))
        provenance={'source_sha256':fingerprint(src)};scope='synthetic shared-context chain through native typed frontend; not eggcc performance'
    engine=ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime'
    feature={'local-sat':'CELL_CPU_LOCAL_SAT','retained':'CELL_CPU_RETAINED_CONTEXTS','spine-prefix':'CELL_CPU_SPINE_PREFIXES','stage-spine':'CELL_CPU_STAGE_SPINE','stage-partial':'CELL_CPU_STAGE_SPINE_PARTIAL','stage-shared':'CELL_CPU_STAGE_SPINE_SHARED_ENTRIES'}[a.feature]
    settings={**SETTINGS,'CELL_CPU_STAGE_SPINE':'1' if a.feature in ('stage-partial','stage-shared') else '0','CELL_CPU_STAGE_SPINE_PARTIAL':'1' if a.feature=='stage-shared' else '0','CELL_CPU_STAGE_SPINE_SHARED_ENTRIES':'0','CELL_CPU_LOCAL_SAT':'0','CELL_CPU_LOCAL_SAT_AUDIT':'0','CELL_CPU_REFUTATION_PROFILE':'0',
        'CELL_CPU_SPINE_PREFIXES':'0','CELL_CPU_RETAINED_CONTEXTS':'1' if a.feature=='spine-prefix' else '0','CELL_CPU_RETAINED_SHARED':'1','CELL_CPU_RETAINED_SETS':a.context_sets}
    old={k:os.environ.get(k) for k in settings}
    result={'scope':scope+'; standalone single-thread Cell CPU, merge programs ON','program':str(path),
        'program_sha256':fingerprint(path),'binary_sha256':fingerprint(engine),
        'feature':a.feature,'shared_ablation':a.shared_ablation,'prefix_ablation':a.prefix_ablation,'stage_ablation':a.stage_ablation,'provenance':provenance,'lanes':{}}
    try:
        os.environ.update(settings)
        with mode(False):
            states=[];controls=[]
            assert not a.shared_ablation or a.feature=='retained'
            lanes=[('off','0','1'),('private','1','0'),('on','1','1')] if a.shared_ablation else [('off','0','1'),('on','1','1')]
            assert not a.prefix_ablation or a.feature=='spine-prefix'
            if a.prefix_ablation:lanes=[('ordinary','0','1'),('off','0','1'),('on','1','1')]
            assert not a.stage_ablation or a.feature in ('stage-partial','stage-shared')
            if a.stage_ablation:lanes=[('ordinary','0','1'),('off','0','1'),('on','1','1')]
            if a.stage_ablation and a.feature=='stage-shared':lanes=[('ordinary','0','1'),('whole','0','1'),('off','0','1'),('on','1','1')]
            for name,flag,shared in lanes:
                if a.stage_ablation and a.feature=='stage-shared':os.environ['CELL_CPU_STAGE_SPINE_PARTIAL']='0' if name in ('ordinary','whole') else '1'
                if a.stage_ablation:os.environ['CELL_CPU_STAGE_SPINE']='0' if name=='ordinary' else '1'
                if a.prefix_ablation:os.environ['CELL_CPU_RETAINED_CONTEXTS']='0' if name=='ordinary' else '1'
                os.environ['CELL_CPU_RETAINED_SHARED']=shared
                os.environ[feature]=flag;m,r=timed(path,engine,out/name)
                states.append(json.loads((out/name/'state.json').read_text()))
                controls.append([{k:s[k] for k in ('ruleset','iterations','path','epoch','logical_matches','changed')} for s in r['steps']])
                result['lanes'][name]={'measurement':m,**metrics(r),'local_saturation':r['local_saturation'],
                    'retained_contexts':r['retained_contexts'],'stage_spine':r['stage_spine'],
                    'runs_and_repair_calls':len(r['steps']),'changed_runs':sum(s['changed'] for s in r['steps']),
                    'point_reads':r['multi_stage']['stats']['point_reads'],
                    'inverse_reads':r['multi_stage']['stats']['inverse_reads'],
                    'merge_port_reads':r['multi_stage']['stats']['merge_port_reads'],
                    'decoded_rows':r['stats']['decoded_rows'],
                    'candidate_scope':'ordinary delta/residual candidates plus merge-program candidates; fixed point reads reported separately',
                    'total_candidates':r['stats']['residual_candidates']+r['multi_stage']['stats']['merge_candidate_rows'],
                    'direct_candidates':r['stats']['direct_candidates'],
                    'all_candidate_visits':r['stats']['direct_candidates']+r['stats']['residual_candidates']+r['multi_stage']['stats']['merge_candidate_rows']+r['stage_spine']['stats']['pivot_candidates'],
                    'all_candidate_scope':'ordinary delta/residual + merge candidates + direct rule seeds + Spine pivot probes; precise port/capture reads separate'}
                stats=r['local_saturation' if a.feature=='local-sat' else ('stage_spine' if a.feature in ('stage-spine','stage-partial','stage-shared') else 'retained_contexts')]['stats']
                print(name,m['inference_seconds'],len(r['steps']),{k:v for k,v in stats.items() if not isinstance(v,dict)},flush=True)
                if a.feature=='spine-prefix':print(name,'prefix',r['retained_contexts']['spine_prefixes']['stats'],flush=True)
            if a.feature!='local-sat':
                assert all(c==controls[0] for c in controls[1:]),'source Run controls differ'
                result['all_lanes_same_run_controls']=True
            result['endpoints']=[compare_native(program['source_core'],s,states[0]) for s in states[1:]]
            result['endpoint']=result['endpoints'][-1]
            result['ratio']=result['lanes']['on']['measurement']['inference_seconds']/result['lanes']['off']['measurement']['inference_seconds']
            (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')
            if a.audit:
                if a.feature!='local-sat':
                    from experiments.cell_runtime.refutation_bench import audit as stage_audit
                    result['verification']=stage_audit(path,program,out/'audit',ROOT/'engine/cell-runtime/target/release/examples/project_stages',feature)
                else:result['verification']=audit(path,program,out/'audit')
                if a.stage_ablation:
                    if a.feature=='stage-shared':
                        os.environ['CELL_CPU_STAGE_SPINE_SHARED_ENTRIES']='0'
                        result['whole_verification']=stage_audit(path,program,out/'whole-audit',ROOT/'engine/cell-runtime/target/release/examples/project_stages','CELL_CPU_STAGE_SPINE_PARTIAL')
                        os.environ['CELL_CPU_STAGE_SPINE_SHARED_ENTRIES']='1'
                    os.environ['CELL_CPU_STAGE_SPINE_PARTIAL']='1'
                    result['ordinary_verification']=stage_audit(path,program,out/'ordinary-audit',ROOT/'engine/cell-runtime/target/release/examples/project_stages','CELL_CPU_STAGE_SPINE')
                # Fresh process reproduces actual identities, not only isomorphism.
                os.environ[feature]='1'
                subprocess.run([str(engine),'--program',str(path),'--output',str(out/'replay')],check=True,capture_output=True)
                assert states[-1]==json.loads((out/'replay/state.json').read_text())
                result['fresh_process_exact_replay']=True
                (out/'results.json').write_text(json.dumps(result,indent=2)+'\n');print('verified',result['verification'].get('boundaries',result['verification'].get('snapshots')),flush=True)
    finally:
        for k,v in old.items():
            if v is None:os.environ.pop(k,None)
            else:os.environ[k]=v
if __name__=='__main__':main()

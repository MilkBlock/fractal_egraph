"""Same-pipeline refutation ablation: off, legacy, scoped maintenance, proof slices."""
import argparse,collections,json,os
from pathlib import Path
from eggfuse.engine.cell_runtime import execute,load_pipeline_artifact
from experiments.cell_runtime.presearch_bench import SETTINGS,ROOT
from experiments.cell_runtime.compositions_bench import mode,metrics
from experiments.cell_runtime.suite import timed,fingerprint
from experiments.cell_runtime.refutation_bench import audit

def depth_summary(lanes,program):
    baseline=lanes['off'];result={};base_rules=baseline['depth']['rules']
    fields=('reads','candidate_rows','refuted_before_depth','deepest_seed')
    base_rows=sum(sum(r['candidate_rows'].values()) for r in base_rules.values())
    for variant,lane in lanes.items():
        rules=lane['depth']['rules'];totals={k:collections.Counter() for k in fields};seeds=0;credit=[]
        assert {r:v['seeds'] for r,v in rules.items()}=={r:v['seeds'] for r,v in base_rules.items()}
        for rid,r in rules.items():
            seeds+=r['seeds'];assert sum(r['deepest_seed'].values())==r['seeds']
            for k in fields:totals[k].update({int(d):v for d,v in r[k].items()})
            saved=sum(base_rules[rid]['candidate_rows'].values())-sum(r['candidate_rows'].values())
            if saved:
                meta=program['recipe']['rules'][int(rid)]['metadata'];stage=meta['source_set']
                group='structural_key' if stage=='cell-stable-keys' or stage.startswith('cell-key-') else 'marker_publisher' if stage=='cell-spine-complete' else 'consumer'
                credit.append({'rule':int(rid),'source_set':stage,'name':meta['name'],'group':group,'saved_candidate_rows':saved})
        saved=base_rows-sum(totals['candidate_rows'].values())
        assert saved==baseline['total_candidates']-lane['total_candidates']
        result[variant]={'seeds':seeds,**totals,'saved_candidates':saved,
            'probability_max_depth_at_least':{d:sum(n for k,n in totals['deepest_seed'].items() if k>=d)/seeds for d in range(1,max(totals['deepest_seed'])+1)},
            'fixed_rule_weights':True,'consumer_credit':sorted(credit,key=lambda r:-r['saved_candidate_rows'])}
    return result

def main():
    p=argparse.ArgumentParser();p.add_argument('--compiled',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--shadow',action='store_true')
    p.add_argument('--profile',action='store_true');p.add_argument('--audit',action='store_true')
    p.add_argument('--variants',nargs='+',default=['off','legacy','scoped','cores','bounds'])
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    path,program,provenance=load_pipeline_artifact(a.compiled.resolve())
    engine=ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime'
    settings={**SETTINGS,'CELL_CPU_REFUTATION_SCOPE':'multistage','CELL_CPU_REFUTATION_VARIANT':'legacy',
        'CELL_CPU_REFUTATION_ENTRIES':'1','CELL_CPU_REFUTATION_PROFILE':'1' if a.profile else '0'}
    old={k:os.environ.get(k) for k in settings};result={'scope':'single-thread standalone Cell CPU; source_sk_mk unchanged, merge programs ON; not pristine egglog',
        'program':str(path),'provenance':provenance,'program_sha256':fingerprint(path),'binary_sha256':fingerprint(engine),'lanes':{}}
    try:
        os.environ.update(settings)
        with mode(False):
            states=[];schedules=[]
            for name in (['shadow'] if a.shadow else a.variants):
                os.environ['CELL_CPU_MARKER_REFUTATION']='0' if name=='off' else '1'
                os.environ['CELL_CPU_REFUTATION_VARIANT']='legacy' if name=='off' else name
                if a.shadow or a.profile:
                    r=execute(program,out/name,engine);data=out/name/'runtime';m={'timing_eligible':False}
                else:m,r=timed(path,engine,out/name);data=out/name
                assert r['status']=='complete' and r['features']['merge_programs']
                result['lanes'][name]={'measurement':m,**metrics(r),
                    'total_candidates':r['stats']['residual_candidates']+r['multi_stage']['stats']['merge_candidate_rows'],
                    'merge_candidate_rows':r['multi_stage']['stats']['merge_candidate_rows'],
                    'refutation':r['marker_refutation'],'depth':r.get('refutation_depth')}
                states.append(json.loads((data/'state.json').read_text()))
                schedules.append([{k:s[k] for k in ('ruleset','path','iterations','epoch','logical_matches','changed')} for s in r['steps']])
                assert all(s==states[0] for s in states) and all(s==schedules[0] for s in schedules)
                stats=r['marker_refutation']['stats']
                print(name,{'seconds':m.get('inference_seconds'),'candidates':result['lanes'][name]['total_candidates'],**{k:stats[k] for k in ('hits','validation_checks','compiled_hits','bound_changed_threshold_hits','admitted_sliced_proofs')}},flush=True)
                (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')
            result['exact_endpoint_and_schedule']=len(states)>1
            if a.profile and not a.shadow:
                result['depth_summary']=depth_summary(result['lanes'],program)
            if a.audit:
                result['verification']={}
                for variant in a.variants:
                    if variant=='off':continue
                    os.environ['CELL_CPU_REFUTATION_VARIANT']=variant
                    result['verification'][variant]=audit(path,program,out/('audit-'+variant),ROOT/'engine/cell-runtime/target/release/examples/project_stages')
                    print(variant,result['verification'][variant],flush=True)
            (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    finally:
        for k,v in old.items():
            if v is None:os.environ.pop(k,None)
            else:os.environ[k]=v
if __name__=='__main__':main()

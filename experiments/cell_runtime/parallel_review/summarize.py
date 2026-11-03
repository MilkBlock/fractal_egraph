"""Summarize a completed profile matrix. Run from the assessed checkout."""
import argparse
import collections
import hashlib
import json
import re
import statistics as stat
import subprocess
from pathlib import Path

def read(path):
    return json.loads(path.read_text())

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def describe(xs):
    return {'median':stat.median(xs),'min':min(xs),'max':max(xs)}

def eggcc(raw):
    samples=[]
    for b in range(1,7):
        for n in (1,4,16):
            d=raw/f'b{b}-t{n}'/'sum-bits'
            m=read(d/'measurement.json')
            assert f'effective Rayon threads={n} ' in (d/'profile.log').read_text()
            passes=[]
            for i in range(1,4):
                p=read(d/f'pass-{i}.json')
                sched=p['schedule_profile']
                passes.append({'input':sha(d/f'pass-{i}.egg'),'nodes':p['nodes'],'classes':p['eclasses'],
                    'matches':p['native_totals']['matches'],'schedule_ms':1000*(sched['saturate_seconds']+sched['run_seconds']),
                    'execution_ms':1000*p['execution_seconds'],'extract_ms':1000*p['extraction_wall_seconds'],
                    **{k.replace('_seconds','_ms'):1000*v for k,v in p['native_totals'].items() if k.endswith('_seconds')}})
            samples.append({'block':b,'threads':n,'source':m['source_sha256'],'binary':m['binary_sha256'],
                            'wall_ms':m['cli_wall_seconds']*1000,'output':sha(d/'program.txt'),'passes':passes})
    clean=read(raw/'clean.json')
    for row in clean:
        report=read(raw/f"clean-b{row['block']}-t{row['threads']}"/'run.json')
        duration=report['eggcc_compile_time']
        row['compiler_ms']=1000*duration['secs']+duration['nanos']/1e6
    assert len(clean)==len(samples)==18
    assert len({s['binary'] for s in samples})==len({s['source'] for s in samples})==1
    assert len(set(r['output'] for r in clean+samples))==1
    for i in range(3):
        assert len({r['passes'][i]['input'] for r in samples})==1
    by_width={}
    for n in (1,4,16):
        subset=[s for s in samples if s['threads']==n]
        cl=[s for s in clean if s['threads']==n]
        by_width[n]={'clean_wall_ms':describe([r['wall']*1000 for r in cl]),
            'compiler_ms':describe([r['compiler_ms'] for r in cl]),
            'process_tree_cpu_over_wall':describe([r['cpu']/r['wall'] for r in cl]),
            'profile_wall_ms':describe([s['wall_ms'] for s in subset]),
            'pass_schedule_ms':[describe([s['passes'][i]['schedule_ms'] for s in subset]) for i in range(3)]}
        for key in ('schedule_ms','execution_ms','extract_ms','search_and_apply_ms','insert_merge_ms','rebuild_ms'):
            by_width[n][key]=describe([sum(p[key] for p in s['passes']) for s in subset])
    return {'schedule_samples':samples,'clean_samples':clean,'by_width':by_width,
            'pool_only_binary':sha(raw/'eggcc/target/release/eggcc'),'profile_tiger':sha(raw/'tiger'),
            'clean_tiger':sha(raw/'eggcc/target/release/tiger'),'lock':sha(raw/'eggcc/Cargo.lock')}

def matrix(rows):
    result={}
    for lane in sorted({r['lane'] for r in rows}):
        group=[r for r in rows if r['lane']==lane]
        assert len({r['checks'] for r in group})==1
        result[lane]={'ms':describe([r['ms'] for r in group]),'checks':group[0]['checks']}
    return result

def span(raw,lane):
    control=raw/'plain-join/1-both' if lane=='lean' else raw/'cell-matrix'/f'1-{lane}'
    clean=read(control/'report.json')
    diag=read(raw/f'span-{lane}'/'report.json')
    assert read(control/'state.json')==read(raw/f'span-{lane}'/'state.json')
    assert len(clean['steps'])==len(diag['steps'])
    phases=collections.Counter()
    tasks=[]
    groups=[]
    for c,s in zip(clean['steps'],diag['steps']):
        assert all(c[k]==s[k] for k in ('ruleset','path','epoch','iterations','logical_matches','changed'))
        phases.update(c['phases'])
        q=[t[1] for t in s['query_tasks']]
        tasks.extend(q);groups.append((c['phases']['search'],q))
    T=clean['inference_seconds'];S=phases['search'];pred={}
    for n in (2,4,8,16):
        lower=0;greedy=0
        for t,q in groups:
            total=sum(q)
            if total==0:lower+=t;greedy+=t;continue
            loads=[0]*n
            for v in sorted(q,reverse=True):loads[loads.index(min(loads))]+=v
            lower+=t*max(total/n,max(q))/total
            greedy+=t*max(loads)/total
        pred[n]={'divisible_search':T/(T-S+S/n),'indivisible_rule_ceiling':T/(T-S+lower),
                 'rule_lpt_zero_overhead':T/(T-S+greedy)}
    us=[t*1e6 for t,_ in groups]
    qs=stat.quantiles(us,n=100,method='inclusive')
    return {'clean_ms':T*1000,'phase_ms':{k:v*1000 for k,v in phases.items()},'runs':len(us),
            'empty_patch_runs':sum(s['logical_matches']==0 for s in clean['steps']),
            'no_change_runs':sum(not s['changed'] for s in clean['steps']),
            'rule_calls':len(tasks),'query_duration_source':'separate sequential instrumented process',
            'search_us':{'median':stat.median(us),'p90':qs[89],'p99':qs[98],'max':max(us)},
            'runs_under_10us':sum(t<10 for t in us),'search_fraction_in_top_10pct_runs':sum(sorted(us,reverse=True)[:len(us)//10])/sum(us),
            'share_rules_under_1us':sum(t<1000 for t in tasks)/len(tasks),'model':pred}

def main():
    ap=argparse.ArgumentParser();ap.add_argument('raw',type=Path);ap.add_argument('output',type=Path);a=ap.parse_args()
    raw=a.raw
    flags=collections.defaultdict(set);lines={}
    for p in sorted(Path('engine/cell-runtime/src').glob('*.rs')):
        s=p.read_text();lines[p.name]=len(s.splitlines())
        for key in re.findall(r'CELL_CPU_[A-Z0-9_]+',s):flags[key].add(p.name)
    replays=read(raw/'replay.json')
    variants=collections.defaultdict(list)
    for row in replays:
        tag=f"{row['workers']}-{'resident' if row['resident'] else 'caller'}-cutoff{row['cutoff']}"
        variants[tag].extend(row['sample_ms'])
    pure={k:describe(v) for k,v in variants.items()}
    samples=read(raw/'cell-matrix/samples.json');joins=read(raw/'join-matrix/samples.json')
    histogram=[len(g) for g in read(raw/'jobs.json').values()]
    probe=read(raw/'span-nodes/report.json')
    program=read(Path('out/shared-node-compile/eggfuse_related_files/program.json'))
    pair_counts=collections.Counter()
    for stage in probe['steps']:
        ids=[rid for rid,_,bindings in stage['query_tasks'] if bindings]
        pair_counts['multi_active_rule_runs']+=len(ids)>1
        for pos, rid in enumerate(ids):
            left=program['recipe']['rules'][rid]['footprint']
            for other in ids[pos+1:]:
                right=program['recipe']['rules'][other]['footprint']
                union=bool(left['union_sorts'] or right['union_sorts'])
                ww=bool(set(left['write_tables']) & set(right['write_tables']))
                rw=bool(set(left['read_tables']) & set(right['write_tables']) or set(right['read_tables']) & set(left['write_tables']))
                pair_counts.update(all_pairs=1,any_union=int(union),write_write=int(ww),read_write=int(rw),any_hazard=int(union or ww or rw))
    depth_path=raw/'join-matrix/depth/results.json'
    depth=read(depth_path if depth_path.exists() else raw/'join-depth/results.json')
    sieve=read(raw/'join-matrix/1-both/report.json')['sieve']
    extra={}
    for directory in ('storage-final','plain-join'):
        samples_=read(raw/directory/'samples.json')
        extra[directory]={'matrix':matrix(samples_),'samples':samples_,'audit':read(raw/directory/'audits.json')}
    output={'assessed_engine_tree':subprocess.check_output(['git','rev-parse','HEAD:engine/cell-runtime'],text=True).strip(),
            'eggcc_revision':subprocess.check_output(['git','-C',raw/'eggcc','rev-parse','HEAD'],text=True).strip(),
            'host':{key:subprocess.check_output(['sysctl','-n',key],text=True).strip() for key in
                    ('machdep.cpu.brand_string','hw.ncpu','hw.perflevel0.physicalcpu','hw.perflevel1.physicalcpu','hw.memsize')},
            'os_version':subprocess.check_output(['sw_vers','-productVersion'],text=True).strip(),
            'additional_ablations':extra,'sieve_work':{k:v for k,v in sieve['stats'].items() if k!='per_rule'},
            'join_depth':{k:depth[k] for k in ('summary','fixed_rule_weight_curves','fixed_rule_weight_common_support_mass','scope')},
            'coactive_rule_pairs':pair_counts,'engine_loc':lines,'flags':{k:sorted(v) for k,v in sorted(flags.items())},
            'eggcc':eggcc(raw),'cell_matrix':matrix(samples),'cell_samples':samples,'join_matrix':matrix(joins),
            'join_samples':joins,'cell_audits':read(raw/'cell-matrix/audits.json'),'join_audit':read(raw/'join-matrix/audits.json'),
            'span':{n:span(raw,n) for n in ('nodes','both','lean')},'entry_replay_ms':pure,'entry_replay_samples':replays,
            'entry_jobs_per_nonempty_run':describe(histogram),'cell_program':sha(Path('out/shared-node-compile/eggfuse_related_files/program.json')),
            'cell_release':sha(Path('engine/cell-runtime/target/release/eggfuse-cell-runtime')),
            'entry_jobs_sha256':sha(raw/'jobs.json'),'cell_projection_binary':sha(raw/'probe/engine/cell-runtime/target/release/examples/entry_replay')}
    a.output.write_text(json.dumps(output,indent=2)+'\n')
if __name__=='__main__':main()

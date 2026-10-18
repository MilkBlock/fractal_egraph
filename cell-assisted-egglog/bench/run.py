#!/usr/bin/env python3
"""Release, same-result benchmark. Timing and strict native verification run separately.
Every result is gated by exact final graph + primitive value equality with pristine.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time
import threading

import psutil

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT));sys.path.insert(0,str(ROOT/'cell-assisted-egglog'))
from tools.bench.math_graph_equal import verify
from validate_numeric import primitive_rows
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
PRIMITIVE={'i64','bool','String','Unit'}

def workload(n,kind):
    consumer='''(function Best (Expr) i64 :merge (min old new))
(relation Offer (Expr Expr i64))
'''
    if kind=='local':
        consumer+='(rule ((Offer dst (Single x) cost) (= old (Best dst)) (< cost old)) ((set (Best dst) (- cost 1))) :name "cost-improve")\n'
    elif kind=='sparse-cycle':
        consumer+='''(relation L1 (Expr Expr))
(relation L2 (Expr Expr))
(relation L3 (Expr Expr))
(rule ((= s (Single x)) (Offer dst s cost) (L1 s a) (L2 a b) (L3 b dst)
       (= old (Best dst)) (< cost old))
      ((set (Best dst) (- cost 1))) :name "cost-improve")
'''
    else:raise ValueError(kind)
    facts=[];stride=n+1
    for i in range(n):
        dst=stride+i
        facts += [f'(Single (Leaf {i}))',f'(set (Best (Leaf {dst})) 100)']
        facts += [f'(Offer (Leaf {dst}) (Single (Leaf {i})) {v})' for v in (3,5,200)]
        if kind=='sparse-cycle':
            target=i if i%31==0 else (i+1)%n
            facts += [f'(L1 (Single (Leaf {i})) (Leaf {2*stride+i}))',
                      f'(L2 (Leaf {2*stride+i}) (Leaf {3*stride+i}))',
                      f'(L3 (Leaf {3*stride+i}) (Leaf {stride+target}))']
    successes=n if kind=='local' else (n+30)//31
    return consumer,'\n'.join(facts)+'\n',successes

def measure(command,prefix,env):
    """perf_counter wall includes launch, all descendants, output and teardown.
    RSS is sampled sum, not unique physical footprint or an exact OS peak.
    """
    begin=time.perf_counter();peak_main=0;peak_tree=0;samples=0
    with prefix.with_suffix('.stdout').open('w') as out,prefix.with_suffix('.stderr').open('w') as err:
        p=subprocess.Popen(list(map(str,command)),cwd=ROOT,env=env,stdout=out,stderr=err,start_new_session=True)
        stop=threading.Event()
        def sample():
            nonlocal peak_main,peak_tree,samples
            while not stop.is_set():
                try:
                    proc=psutil.Process(p.pid)
                    main=proc.memory_info().rss;tree=main
                    for child in proc.children(recursive=True):
                        try:tree+=child.memory_info().rss
                        except psutil.NoSuchProcess:pass
                    peak_main=max(peak_main,main);peak_tree=max(peak_tree,tree);samples+=1
                except psutil.NoSuchProcess:return
                stop.wait(.01)
        sampler=threading.Thread(target=sample,daemon=True);sampler.start()
        try:
            code=p.wait()
            wall=time.perf_counter()-begin
        except BaseException:
            import signal
            os.killpg(p.pid,signal.SIGKILL);p.wait();raise
        finally:stop.set();sampler.join()
    if code:raise RuntimeError(f'command failed ({code}): {command}\n{prefix.with_suffix(".stderr").read_text()[-2000:]}')
    return dict(command=list(map(str,command)),process_wall_seconds=wall,
        sampled_peak_main_rss_bytes=peak_main or None,sampled_peak_process_tree_rss_bytes=peak_tree or None,
        rss_samples=samples,result=json.loads(prefix.with_suffix('.stdout').read_text()))

def check_graph(reference,current,out):
    a=json.loads(reference.read_text());b=json.loads(current.read_text())
    eq={r['sort'] for r in a if r['sort'] not in PRIMITIVE}
    if primitive_rows(a,eq)!=primitive_rows(b,eq):raise AssertionError('primitive values/visibility differ')
    paths=[]
    for label,rows in [('a',a),('b',b)]:
        p=out.with_name(out.name+f'-{label}.json')
        p.write_text(json.dumps([r for r in rows if r['sort'] in eq]));paths.append(p)
    result=verify(*paths,eq_sorts=eq)
    # Attach a comparison-only primitive observation to each exact row key.
    # Merely comparing per-table counts would miss swapped subsumption flags.
    def visibility(rows):
        probes=[dict(op='visibility:'+r['op'],args=r['args'],sort='bool',result=str(r['subsumed']),subsumed=False) for r in rows]
        return primitive_rows(rows+probes,eq)
    if visibility(a)!=visibility(b):
        raise AssertionError('table visibility differs')
    result.update(primitive_values_exact=True,total_rows=len(a))
    return result

def assert_timed(result,assisted):
    if result.get('graph_snapshot_requested') is not False:raise AssertionError('timing included graph export')
    for key in ['setup_seconds','input_seconds','run_seconds','core_seconds']:
        if result.get(key) is None or result[key]<0:raise AssertionError(f'missing timing {key}')
    if abs(result['core_seconds']-sum(result[k] for k in ['setup_seconds','input_seconds','run_seconds']))>1e-8:
        raise AssertionError('inconsistent phase accounting')
    if assisted:
        s=result['stats']
        if s['verification']['enabled'] or s['verification']['batches'] or s['audit_uncovered']:
            raise AssertionError('timed assisted run enabled native oracle')
        phases=s['phase_seconds'];total=sum(v for k,v in phases.items() if k!='device_seconds')
        if total>result['run_seconds']*1.01+1e-6:raise AssertionError('phase timers overlap')
    if result['progress'][-1]:raise AssertionError('fixed round count did not reach a quiet boundary')

def check_expected_best(path,n,kind):
    rows=json.loads(path.read_text())
    leaves={r['result']:int(r['args'][0][1]) for r in rows if r['op']=='Leaf'}
    values={leaves[r['args'][0][1]]:int(r['result']) for r in rows if r['op']=='Best'}
    expected={n+1+i:(2 if kind=='local' or i%31==0 else 100) for i in range(n)}
    if values!=expected:raise AssertionError('independent expected Best values differ')

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--build',type=Path,required=True);ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--layout-python',type=Path,required=True);ap.add_argument('--solver',choices=['scip','gurobi'],default='scip')
    ap.add_argument('--sizes',default='16,64,256');ap.add_argument('--cases',default='local,sparse-cycle');ap.add_argument('--threads',type=int,default=4)
    ap.add_argument('--legacy-small',action='store_true',help='also time old Cartesian/Gram on the fixed 16-instance three-table local diagnostic')
    a=ap.parse_args();sizes=[int(x) for x in a.sizes.split(',')]
    if not sizes or min(sizes)<1 or a.threads<1:ap.error('positive sizes/threads required')
    if a.legacy_small and (16 not in sizes or 'local' not in a.cases.split(',')):
        ap.error('--legacy-small requires the local case at size 16; Cartesian is not run on larger/multi-table workloads')
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    manifest=json.loads(a.build.read_text());bins={k:Path(v['path']) for k,v in manifest['binaries'].items()}
    for k,b in bins.items():
        if sha(b)!=manifest['binaries'][k]['sha256']:raise ValueError(f'binary changed since build: {k}')
    env=dict(os.environ,RAYON_NUM_THREADS=str(a.threads),EGGLOG_PROBE_COUNTS_OFF='1')
    for key in ['EGG_LAYOUT_FULL_TRACE','EGG_LAYOUT_PROFILE','EGG_LAYOUT_RIPEN_HISTORY']:env.pop(key,None)
    analysis=ROOT/'cell-assisted-egglog/fixtures/analyze'
    library={str(p.relative_to(analysis)):dict(bytes=p.stat().st_size,sha256=sha(p)) for p in sorted(analysis.rglob('*.json'))}
    report=dict(schema='cell-layout-bench/v1',completed=False,platform=platform.platform(),build=manifest,library=library,
        library_file_bytes=sum(x['bytes'] for x in library.values()),threads=a.threads,runs_per_variant=1,cases=[],
        notes=['Single/Get Cell comes from real analyze; Offer/Best and sparse-cycle consumers are controlled workloads, not a full eggcc pass.',
        'All binaries release; verification and graph dumps are separate fresh processes; build and old offline Bake are excluded.',
        'Core includes setup/input/all driver rounds; run includes event/marker maintenance, solving, shader install, transfer, GPU execution and native commits.',
        'Process wall includes startup/report/teardown; wait is independent of the RSS sampler. GPU device time is only a subset of dispatch time.',
        'RSS is a 10ms sampled sum over currently live processes; can miss peaks and double-count shared pages. It is not GPU buffer size.',
        'One fresh process per variant; OS/Metal compiler caches are not flushed. No averaging or confidence interval; sub-millisecond differences are not stable speedups.',
        'The old Cartesian assistant is only a small local-case diagnostic, never the native reference. Multi-table cases compare pristine/fork with indexed variants.'])
    def save():(out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    save()
    for kind in a.cases.split(','):
      for n in sizes:
        case=out/f'{kind}-{n}';case.mkdir()
        extension,input_text,successes=workload(n,kind)
        source=case/'consumer.egg';source.write_text(extension);data=case/'input.egg';data.write_text(input_text)
        common=[bins['assisted'],'--analysis',analysis,'--consumer-source',source,'--ruleset','','--consumer','cost-improve']
        environment=case/'environment.egg'
        environment.write_text(subprocess.check_output(list(map(str,common+['--mode','environment'])),cwd=ROOT,text=True,env=env))
        inspect=json.loads(subprocess.check_output(list(map(str,common+['--mode','inspect'])),cwd=ROOT,text=True,env=env))
        parts=[dict(atoms=[row['id']],cell=row['table']=='Single') for row in inspect['atoms']]
        base=common+['--parts',json.dumps(parts),'--native','single-get','--input',data,'--rounds','4','--gpu-worker',bins['worker'],
            '--layout-python',a.layout_python,'--layout-solver',a.solver,'--audit','false']
        variants=[('pristine',None),('fork',None),('marker-paired',['indexed','paired']),('marker-solver',['indexed','solver'])]
        if a.legacy_small and n==16 and kind=='local':variants += [('cartesian-gram',['cartesian','legacy']),('marker-gram',['indexed','legacy'])]
        entry=dict(kind=kind,instances=n,lhs_atoms=len(parts),expected_effective_bindings=2*successes,expected_native_sets=successes,
            input_sha256=sha(data),environment_sha256=sha(environment),parts=parts,runs=[])
        report['cases'].append(entry)
        for name,config in variants:
            cmd=[bins[name],environment,data,'4'] if config is None else base+['--mode','matrix-numeric','--marker-layout',config[0],'--matrix-layout',config[1],'--gpu-check','off','--report-detail','summary']
            try:
                timing=measure(cmd,case/f'{name}-timed',env);assert_timed(timing['result'],config is not None)
            except Exception as error:
                entry.setdefault('failures',[]).append(dict(name=name,phase='timing',error=str(error)));save();raise
            graph=case/f'{name}-graph.json'
            checkcmd=cmd+[graph] if config is None else base+['--mode','matrix-numeric','--marker-layout',config[0],'--matrix-layout',config[1],'--gpu-check','on','--report-detail','summary','--dump',graph]
            try:
                validation=measure(checkcmd,case/f'{name}-verify',env)
                same=check_graph(case/'pristine-graph.json',graph,case/f'{name}-eq')
                check_expected_best(graph,n,kind)
                if config is not None:
                    for run in [timing,validation]:
                        stats=run['result']['stats']
                        if stats['accepted_bindings']!=2*successes or stats['native_sets']!=successes:raise AssertionError('different work/result')
                    if validation['result']['stats']['verification']['batches']==0:raise AssertionError('verification did not execute')
                elif timing['result']['native_matches']!=validation['result']['native_matches']:
                    raise AssertionError('timed/verification native match counts differ')
            except Exception as error:
                entry.setdefault('failures',[]).append(dict(name=name,phase='verification',error=str(error)));save();raise
            entry['runs'].append(dict(name=name,timing=timing,validation=dict(seconds=validation['process_wall_seconds'],graph=same,
                strict_gpu_check=config is not None,stats=validation['result'].get('stats',{}).get('verification'))))
            save();print(f'{kind} n={n} {name}: run={timing["result"]["run_seconds"]:.6f}s core={timing["result"]["core_seconds"]:.6f}s verified',flush=True)
    for case in report['cases']:
        baseline=case['runs'][0]['timing']['result']
        for run in case['runs']:
            r=run['timing']['result'];run['pristine_over_variant_core']=baseline['core_seconds']/r['core_seconds'];run['pristine_over_variant_run']=baseline['run_seconds']/r['run_seconds']
    report['harness_sha256']=sha(Path(__file__))
    report['completed']=True;save();print(out/'report.json')

if __name__=='__main__':main()

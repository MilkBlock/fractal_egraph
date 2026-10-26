"""Real native rules -> certified ridge/matrix iteration -> native comparison."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import time
import numpy as np
from compiler import compile_context, reference

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]

def native_adapter():
    # Load the existing exact multi-sort graph comparator without replacing this
    # experiment's compiler module with the device backend's same-named module.
    spec=importlib.util.spec_from_file_location('device_compiler',HERE.parent/'device_backend/compiler.py')
    sys.path.insert(0,str(HERE.parent/'device_backend'))
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    previous=sys.modules.get('compiler');sys.modules['compiler']=module
    try:
        spec=importlib.util.spec_from_file_location('device_run',HERE.parent/'device_backend/run.py')
        runner=importlib.util.module_from_spec(spec);spec.loader.exec_module(runner)
    finally:
        if previous is not None:sys.modules['compiler']=previous
    return module.compile_program,runner.compare

def run(source,frontend,worker,out,ruleset,control=None):
    out.mkdir(parents=True,exist_ok=False)
    with (out/'native.log').open('w') as log:
        native=subprocess.Popen([str(frontend.resolve()),str(source.resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log,text=True)
        def ask(op):
            native.stdin.write(json.dumps(op)+'\n');native.stdin.flush()
            line=native.stdout.readline()
            if not line:raise RuntimeError('native oracle stopped; see native.log')
            return json.loads(line)
        try:
            line=native.stdout.readline()
            if not line:raise RuntimeError('native compile failed; see native.log')
            core=json.loads(line);prefix=0;warm_start=time.perf_counter()
            if control:
                saved=json.loads((control/'report.json').read_text())
                if saved['source_sha256']!=hashlib.sha256(source.read_bytes()).hexdigest():raise ValueError('source differs from saved schedule')
                prefix=next(i for i,s in enumerate(saved['steps']) if s['set']==ruleset)
                for i,s in enumerate(saved['steps'][:prefix]):
                    r=ask({'op':'run','set':s['set']})
                    if r['updated']!=s['updated']:raise ValueError(f'native schedule differs before closure at {i+1}')
                core['initial']=ask({'op':'snapshot'})['rows']
            warm_seconds=time.perf_counter()-warm_start
            started=time.perf_counter();program=compile_context(core,ruleset)
            preparation=time.perf_counter()-started
            (out/'program.json').write_text(json.dumps(program))
            print('compiled',len(program['rules']),'rules,',len(program['coordinates']),'fact coordinates,',program['clauses'],'clauses',flush=True)
            # The GPU runs to its own fixed point before the native reference
            # executes any selected closure Run or supplies its expected labels.
            with (out/'gpu.log').open('w') as gl:
                worker_started=time.perf_counter()
                subprocess.run([str(worker.resolve()),str(out/'program.json'),str(HERE/'kernel.metal'),str(out/'birth.bin'),str(out/'gpu.json')],check=True,stdout=gl,stderr=gl)
                worker_wall=time.perf_counter()-worker_started
            gpu=json.loads((out/'gpu.json').read_text())
            birth=np.fromfile(out/'birth.bin',dtype='<u4')
            if len(birth)!=len(program['coordinates']):raise AssertionError('incomplete GPU state')
            expected=reference(program)
            if len(expected)!=gpu['runs']:raise AssertionError('matrix fixed-point boundary differs')
            control_time=ask({'op':'time_closure','set':ruleset})
            if control_time['runs']!=gpu['runs']:raise AssertionError('native timing control frontier differs')
            checks=[];native_seconds=0.;matches=0
            tables=sorted({c['table'] for c in program['coordinates']})
            for step in range(1,gpu['runs']+1):
                visible=(birth<=step).astype(np.uint32).tolist()
                if visible!=expected[step-1]:raise AssertionError('matrix iteration differs from exact finite operator')
                r=ask({'op':'run','set':ruleset,'timed':True});native_seconds+=r['step_seconds'];matches+=r['logical_matches']
                previous=program['initial'] if step==1 else expected[step-2]
                if r['updated']!=(visible!=previous):raise AssertionError('Run visibility/frontier differs')
                view=ask({'op':'unary_predicates','tables':tables})
                decoded={t:sorted(c['input']['eq'] for c,v in zip(program['coordinates'],visible) if v and c['table']==t) for t in tables}
                if decoded!=view:raise AssertionError(f'native predicate facts differ at Run {step}')
                checks.append({'run':step,'new_facts':sum(visible)-sum(previous),'exact_predicates':True,'updated':r['updated']})
            final=ask({'op':'snapshot'})
            materialize_started=time.perf_counter()
            compile_rows,compare=native_adapter()
            # Reuse the repository's typed row/identity encoding only; its
            # generated matcher/shader is not executed by this experiment.
            carrier=dict(core,rulesets=[])
            encoded=compile_rows(carrier,capacity=len(core['initial'])+len(birth)+1,match_capacity=1)
            stride=encoded['stride'];flat=encoded['initial_rows'];rows=[flat[i:i+stride] for i in range(0,len(flat),stride)]
            ids={tuple(a):i+1 for i,a in enumerate(encoded['anchors'])};table_ids={t['name']:i for i,t in enumerate(encoded['tables'])}
            next_id=len(ids)+1
            for c,b in zip(program['coordinates'],birth):
                if b==0 or b==np.iinfo(np.uint32).max:continue
                key=ids[(c['input']['sort'],c['input']['eq'])]
                rows.append([table_ids[c['table']],0,int(b),len(rows)+1,key,next_id]+[0]*(stride-6));next_id+=1
            # This admitted rule class never modifies an initial class. Pin the
            # frozen carrier identities, including those created in the prefix.
            final['anchors']=[{'sort':s,'old':v,'now':v} for s,v in encoded['anchors']]
            materialize_seconds=time.perf_counter()-materialize_started
            full=compare(encoded,rows,list(range(next_id)),final,encoded['initial_sets'])
            report={'scope':'actual eggcc rule family on a frozen structural carrier; standalone matrix executor, not full Pass 3',
                'source':str(source),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
                'ruleset':ruleset,'rules':len(program['rules']),'native_prefix_runs':prefix,
                'native_prefix_seconds':warm_seconds,'operator_preparation_seconds':preparation,
                'worker_wall_including_setup_seconds':worker_wall,
                'final_row_encoding_seconds':materialize_seconds,'native_timing_control':control_time,
                'initial_rows':len(core['initial']),'fact_coordinates':len(birth),'clauses':program['clauses'],
                'anchor_rows_read':program['anchor_rows_read'],'separators':[g['separator'] for g in program['groups']],
                'gpu':gpu,'native_closure_step_seconds_diagnostic':native_seconds,
                'native_logical_matches':matches,'per_run_checks':checks,'full_graph_check':full,
                'final_materialization':'typed rows decoded once after convergence; no native repair or rule execution',
                'matching_predicates_read_during_grounding':0,'weight_nonzero_fraction':0.125,
                'model_labels':'Boolean conjunction truth domains, not expected graph outcomes',
                'limits':['Fixed topology: no in-loop union, set, subsume or structural growth.',
                          'Anchor scan and fixed wiring construction are real preparation costs.',
                          'Block-diagonal weights still perform zero multiplications; no high-utilization claim.',
                          'Native phase timings include intermittent observation; not an end-to-end speedup benchmark.']}
            (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
            print(json.dumps({k:report[k] for k in ['rules','fact_coordinates','clauses','native_logical_matches','full_graph_check']},indent=2))
            return report
        finally:
            if native.poll() is None:
                native.stdin.write('{"op":"quit"}\n');native.stdin.flush();native.wait(timeout=30)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--frontend',type=Path,required=True)
    p.add_argument('--worker',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--ruleset',default='is-resolved');p.add_argument('--control',type=Path)
    a=p.parse_args();run(a.source,a.frontend,a.worker,a.output,a.ruleset,a.control)

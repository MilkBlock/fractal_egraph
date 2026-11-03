"""Build/load the native CPU engine and compare complete typed endpoints."""
import argparse,hashlib,json,subprocess,sys,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]

def binary():
    subprocess.run(['cargo','build','--offline','--release','--manifest-path',str(ROOT/'engine/cpu/Cargo.toml')],cwd=ROOT,check=True)
    return ROOT/'engine/cpu/target/release/eggfuse-cpu'

def compare_native(core,candidate,reference):
    # Adapt only the test snapshot representation, never the execution engine.
    # Reuse the existing exact anchored typed graph comparator.
    sys.path.insert(0,str(ROOT/'experiments/device_backend'))
    from run import compare
    ids={};parents=[0];anchors=[]
    for a in candidate['anchors']:
        i=len(parents);key=(a['sort'],a['now']);ids.setdefault(key,i);parents.append(ids[key]);anchors.append([a['sort'],a['old']])
    strings=set()
    def scan(v):
        if 'elements' in v:
            for e in v['elements']:scan(e)
        elif 'eq' in v:
            k=(v['sort'],v['eq'])
            if k not in ids:ids[k]=len(parents);parents.append(len(parents))
        elif v['sort']=='String':strings.add(v['value'])
    for row in candidate['rows']:
        for v in row['values']:scan(v)
    strings=sorted(strings);string_ids={s:i for i,s in enumerate(strings)};arena=[2,0];key_values=[]
    def value(v):
        if 'key' in v:
            i=len(key_values);key_values.append(v['key']);return i
        if 'eq' in v:return ids[(v['sort'],v['eq'])]
        if 'elements' in v:
            elems=sorted(value(x) for x in v['elements']);h=len(arena);arena.extend([len(elems),len(elems),0,0,*elems]);arena[0]=len(arena);return h
        if v['sort']=='String':return string_ids[v['value']]
        return 0 if v['value'] is None else int(v['value'])
    tables=core['tables'];tids={t['name']:i for i,t in enumerate(tables)};stride=4+max(len(t['schema']) for t in tables);rows=[]
    for row in candidate['rows']:
        r=[tids[row['table']],int(row['subsumed']),0,len(rows)+1,*map(value,row['values'])];r += [0]*(stride-len(r));rows.append(r)
    return compare(dict(tables=tables,strings=strings,anchors=anchors,stride=stride,key_values=key_values),rows,parents,reference,arena)

def execute(source,output,engine,check=False,plan_path=None,mode="cell",stage_audit=False):
    started=time.perf_counter()
    output.mkdir(parents=True,exist_ok=False);source=source.resolve()
    core_file=output/'core.json'
    subprocess.run([str(engine),'--source',str(source),'--prepare',str(core_file)],check=True)
    core=json.loads(core_file.read_text());prepare_seconds=time.perf_counter()-started;compile_started=time.perf_counter()
    if plan_path is None:
        from eggfuse.compiler.targets.cpu import lower
        plan=lower(core,staged=(mode=='staged'));plan_path=output/'cpu-program.json';plan_path.write_text(json.dumps(plan))
    else:
        plan_path=plan_path.resolve();plan=json.loads(plan_path.read_text())
        from eggfuse.ir.recipe import validate
        from eggfuse.analyzer.regions import verify
        validate(plan['recipe']);verify(plan['regions'],plan['recipe'])
        if plan['core']!=core:raise ValueError('CPU program differs from typed input')
    recipe_compile_seconds=time.perf_counter()-compile_started
    def run(mode):
        folder=output/mode;cmd=[str(engine),'--source',str(source),'--output',str(folder),'--mode',mode]
        if mode!='native':cmd+=['--plan',str(plan_path)]
        if mode=='staged' and stage_audit:cmd+=['--stage-audit']
        with (output/(mode+'.log')).open('w') as log:subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT,check=True)
        return json.loads((folder/'report.json').read_text())
    candidate=run(mode);report=dict(schema='eggfuse-cpu-comparison/v1',source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),binary_sha256=hashlib.sha256(engine.read_bytes()).hexdigest(),candidate=candidate,scope='actual native egglog CPU execution; no GPU, no isolated-egraph replay',verification=None)
    if check:
        native=run('native');report['native']=native
        if not native['full_typed_snapshot'] or not candidate['full_typed_snapshot']:raise ValueError('full typed endpoint export unavailable; cannot claim equivalence')
        report['verification']=compare_native(core,json.loads((output/mode/'state.json').read_text()),json.loads((output/'native/state.json').read_text()))
        report['speedup']=native['inference_seconds']/candidate['inference_seconds']
    report.update(prepare_seconds=prepare_seconds,recipe_compile_seconds=recipe_compile_seconds,driver_wall_seconds=time.perf_counter()-started)
    (output/'report.json').write_text(json.dumps(report,indent=2)+'\n');return report

def main(args):
    p=argparse.ArgumentParser(prog='eggfuse cpu');p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--plan',type=Path);p.add_argument('--cpu-bin',type=Path);p.add_argument('--check',action='store_true');p.add_argument('--execution',choices=['eager','staged'],default='eager');p.add_argument('--stage-audit',action='store_true');a=p.parse_args(args)
    engine=a.cpu_bin.resolve() if a.cpu_bin else binary()
    r=execute(a.source,a.output.resolve(),engine,a.check,a.plan,'staged' if a.execution=='staged' else 'cell',a.stage_audit)
    print(json.dumps(dict(inference_seconds=r['candidate']['inference_seconds'],cells=r['candidate']['cells'],staged=r['candidate'].get('staged'),verification=r['verification'],speedup=r.get('speedup')),indent=2));return 0

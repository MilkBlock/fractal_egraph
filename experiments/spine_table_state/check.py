"""Exercise the generated .egg and resident GPU without a Spine side cache."""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import sys
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT),str(ROOT/'experiments/device_backend')]
from eggfuse.compiler.test_spine_states import CASES,BASE,compile_case,TOOLS
from eggfuse.compiler.verify import compare_source
from eggfuse.analyzer.regions import from_core
from eggfuse.engine.regions import verify_repeat
from run_generic_cells import run_library
from verify_native_source import verify
import run_generic_cells as runtime


def main():
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--only',nargs='*');p.add_argument('--pass3-source',type=Path);p.add_argument('--analysis',type=Path);a=p.parse_args()
    a.output.mkdir(parents=True,exist_ok=False);summary={}
    cases=dict(CASES)
    cases['naive-revisit']=BASE.replace(':name "left")',':name "left" :naive)').replace(':name "right")',':name "right" :naive)')+'''(Seed (N 1))
(run build 1) (run late 1) (run consume 1) (run consume 1) (run consume 1)'''
    cases['intermediate-entry']='''(datatype E (N i64) (A E) (B E) (C E) (D E))
(relation Seed (E)) (ruleset build) (ruleset mid) (ruleset finish)
(rule ((Seed x)) ((B (A x))) :ruleset build :name "build")
(rule ((= r (B (A (N n))))) ((C r)) :ruleset mid :name "mid-one")
(rule ((= r (B (A (N n))))) ((D r)) :ruleset mid :name "mid-two")
(rule ((= c (C (B (A (N n)))))) ((D c)) :ruleset finish :name "finish-one")
(rule ((= c (C (B (A (N n)))))) ((C c)) :ruleset finish :name "finish-two")
(B (A (N 3))) (run mid 1) (run finish 1)'''
    for name,source in cases.items():
        if a.only and name not in a.only:continue
        folder=a.output/name;report,core,ir=compile_case(source,folder)
        for tag,path in [('source',folder/'source.egg'),('marked',folder/'compiled/published.egg')]:
            subprocess.run([str(TOOLS/'eggfuse_frontend'),str(path.resolve()),'--native-full','--export-to',str((folder/tag).resolve())],check=True,stdout=subprocess.DEVNULL)
        original=json.loads((folder/'source/native-state.json').read_text());marked=json.loads((folder/'marked/native-state.json').read_text())
        source_check=compare_source(original,marked,ir)
        plan=from_core(core,[m['name'] for m in ir['markers']]);initial=core.pop('initial')
        lib=dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])
        runs=[run_library(copy.deepcopy(lib),a.build,folder/suffix,capacity=1024,match_capacity=4096,marker_program=ir,region_program=plan,audit_numeric=True) for suffix in ('gpu','repeat')]
        repeated=verify_repeat(*runs)
        gpu_check=verify(folder/'compiled/published.egg',a.build,folder/'gpu/program-0/cell-0',folder/'gpu-native')
        program=json.loads((folder/'gpu/program-0/cell-0/program.json').read_text())
        assert program['spine_cache_execution']['enabled'] is False
        item=dict(source=source_check,gpu=gpu_check,repeatability=repeated,markers=len(ir['markers']),states=len(ir['continuations']),receipt_shortcuts=program['marker_program']['receipt_shortcuts'])
        if name=='naive-revisit':
            lower=runtime.lower
            with patch.object(runtime,'lower',lambda *args,**kw:lower(*args,**kw,spine_receipt_shortcuts=False)):
                off=run_library(copy.deepcopy(lib),a.build,folder/'receipt-off',capacity=1024,match_capacity=4096,marker_program=ir,region_program=plan,audit_numeric=True)
            verify(folder/'compiled/published.egg',a.build,folder/'receipt-off/program-0/cell-0',folder/'off-native')
            on=runs[0]['programs'][0]['instances'][0];before=off['programs'][0]['instances'][0]
            set_id=program['sets'].index('consume')
            item['consumer_bindings_on']=[h[2] for h in on['history'] if h[0]==set_id]
            item['consumer_bindings_off']=[h[2] for h in before['history'] if h[0]==set_id]
            assert item['consumer_bindings_on']==[2,0,0],item
            assert item['consumer_bindings_off']==[2,2,2],item
        summary[name]=item;(a.output/'results.json').write_text(json.dumps(summary,indent=2)+'\n')
        print(name,'native projection + GPU endpoint + L1 passed',flush=True)
    if a.pass3_source:
        from tools.cell_spine.pass3_markers import compile_source
        from eggfuse.compiler.pipeline import native_core
        from eggfuse.compiler.marker_ir import build
        from eggfuse.compiler.targets.metal.generic_cell import lower
        if not a.analysis:p.error('--pass3-source requires saved --analysis')
        folder=a.output/'pass3'
        report=compile_source(a.pass3_source.resolve(),a.analysis.resolve(),folder,TOOLS/'lhs_ast')
        source=folder/'published.egg';core=native_core(TOOLS/'eggfuse_frontend',source.resolve(),folder)
        ir=build(report,core,source.read_bytes(),(a.analysis/'cell-recipes.json').read_bytes())
        plan=from_core(core,[m['name'] for m in ir['markers']]);program=lower(core,capacity=1048576,match_capacity=262144,marker_program=ir,region_program=plan)
        for name,value in [('marker-program',ir),('region-program',plan),('core',core)]: (folder/(name+'.json')).write_text(json.dumps(value,indent=2))
        summary['pass3']=dict(selection=report['selection_diagnostics'],markers=len(ir['markers']),continuations=len(ir['continuations']),immutable_receipts=sum(s['completion']=='immutable-constructor-effects' for s in ir['continuations']),publish_actions=report['variants']['published']['publish_actions'],consumers=report['modified_consumers'],source_bytes=source.stat().st_size,row_stride=program['stride'],source_sha256=ir['source_sha256'],device_lowering=True,full_pass3_executed=False)
        (a.output/'results.json').write_text(json.dumps(summary,indent=2)+'\n');print('pass3',summary['pass3'],flush=True)


if __name__=='__main__':main()

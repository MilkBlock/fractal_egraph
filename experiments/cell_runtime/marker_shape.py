"""Pinned source-level ablation. Does not change the general marker solver.

Remove the same-index Get Cartesian view, optionally replacing its If consumers
with a context-gated Unit function. Consumers retain the actual If read because
eggcc may subsume If. The experimental function has no Cell proof authority.
"""
import argparse,copy,json,shutil,subprocess
from pathlib import Path
from eggfuse.compiler.passes import PassManager,verified_receipts,digest
from eggfuse.compiler.pipeline import native_core
from eggfuse.compiler.marker_ir import build as markers_ir,validate as validate_markers
from eggfuse.ir.recipe import build as recipe_ir
from eggfuse.compiler.targets.cell_cpu import compile_target
from eggfuse.engine.cell_runtime import load_pipeline_artifact
from tools.cell_spine import pass3_markers as pm
from tools.cell_spine import takeover as t
from tools.cell_spine.terms import text

OLD='CellSpineView13'
NEW='ExperimentIfGetPair'
ROOT=Path(__file__).resolve().parents[2]

def marker_call(f):
    if f['kind']=='fact' and f['expr'].get('head')==OLD:return f['expr']
    return None

def if_calls(body):
    for f in body:
        for e in f['args'] if f['kind']=='eq' else [f['expr']]:
            if e['kind']=='call' and e['head']=='If':yield e

def transform(data,report,source,mode):
    if mode=='baseline':
        header=[line for line in source.splitlines() if line.startswith('; cell-structural-key-runtime/v1 ')]
        out=copy.deepcopy(report);out['experimental_shape']={'mode':mode,'source_roundtrip_only':True}
        return '\n'.join(header+[c['text'] for c in data['commands']])+'\n',out
    view=next(v for v in report['markers'] if v['name']==OLD)
    assert [f['args'][1]['head'] for f in view['body']]==['Get','Get']
    assert view['sorts']==['Expr','Expr','i64','Expr','Expr']
    fields=view['parameters']
    assert [(f['args'][0]['name'],[a['name'] for a in f['args'][1]['args']]) for f in view['body']]==[(fields[0],[fields[1],fields[2]]),(fields[3],[fields[4],fields[2]])]
    rules={r['command_ordinal']:r for r in data['rules']}
    rewrites={};changed=[];publications=0
    for command,r in rules.items():
        if r['name']=='complete-'+OLD:continue
        body=[];replacements=[]
        for f in r['body']:
            call=marker_call(f)
            if call is None:body.append(f);continue
            a=call['args'];assert len(a)==5
            chosen=None
            if mode=='if' and not r['ruleset'].startswith('cell-'):
                for parent in if_calls(r['body']):
                    thn,els=parent['args'][2:4]
                    if (text(a[1]),text(a[4]))==(text(thn),text(els)):chosen=a;break
                    if (text(a[4]),text(a[1]))==(text(thn),text(els)):chosen=[a[3],a[4],a[2],a[0],a[1]];break
            if chosen is not None:
                body.append(dict(kind='fact',expr=t.call(NEW,chosen)))
                replacements.append('if-gated')
            else:
                binding=dict(zip(view['parameters'],a))
                body.extend(t.remap_fact(g,binding) for g in view['body'])
                replacements.append('expanded')
        head=[a for a in r['head'] if not(a['kind']=='set' and a['head']==OLD)]
        publications+=len(r['head'])-len(head)
        if replacements or head!=r['head']:
            rewrites[command]=pm.render_rule(r,body,head)
            changed.append(dict(rule=r['name'],ruleset=r['ruleset'],replacements=replacements))
    lines=[line for line in source.splitlines() if line.startswith('; cell-structural-key-runtime/v1 ')]
    for c in data['commands']:
        r=rules.get(c['ordinal'])
        if c['text'].startswith('(function '+OLD+' '):
            if mode=='if':lines.append(f'(function {NEW} (Expr Expr i64 Expr Expr) Unit :merge old)')
        elif r and r['name']=='complete-'+OLD:
            if mode=='if':
                lines.append(f'(rule ((If p inputs thn els) (= u (Get thn i)) (= v (Get els i))) ((set ({NEW} u thn i v els) ())) :ruleset {pm.REPAIR} :name "experiment-if-get-complete")')
        else:lines.append(rewrites.get(c['ordinal'],c['text']))
    transformed='\n'.join(lines)+'\n'
    out=copy.deepcopy(report);out['markers']=[v for v in out['markers'] if v['name']!=OLD]
    # Existing longer guarantees are unchanged. Their completion queries now
    # expand the removed prefix; the shared IR validator independently checks it.
    for v in out['markers']:
        if v.get('parent')==OLD:v['parent']=None
        body=[]
        for f in v['completion_body']:
            call=marker_call(f)
            if call is None:body.append(f)
            else:body.extend(t.remap_fact(g,dict(zip(view['parameters'],call['args']))) for g in view['body'])
        v['completion_body']=body
    out['experimental_shape']={'mode':mode,'removed':OLD,'conditional_view':NEW if mode=='if' else None,
        'original_if_guard_retained':True,'conditional_view_is_not_a_certified_marker':True,
        'removed_publications':publications,'changed_rules':changed}
    out['cost_model']='pinned experiment; existing solver selection otherwise retained'
    return transformed,out

def compile_variant(source,dest,mode,frontend,ast):
    source=source.resolve();dest=dest.resolve()
    _,_,provenance=load_pipeline_artifact(source)
    pipeline=json.loads((source/'pipeline.json').read_text())
    retained=[s['name'] for s in pipeline['passes'] if s['name'] in ('structural_keys','bake_environment','analyze')]
    records=verified_receipts(source,retained+['markers'])
    manager=PassManager(dest)
    for step in pipeline['passes']:
        if step['name'] not in retained:continue
        old=records[step['name']][0];new=dest/'passes'/step['name']/step['key']
        new.parent.mkdir(parents=True,exist_ok=True);shutil.copytree(old,new)
        manager.receipts.append({**step,'directory':str(new)})
    for name in ('source.egg','source_sk.egg'):manager.publish(source/name,name)
    manager.save()
    analysis=records['analyze'][0]/'result/cell-recipes.json'
    baseline_report=records['markers'][0]/'result/markers.json'
    def emit(folder):
        result=folder/'result';result.mkdir()
        subprocess.run([str(ast),str(source/'source_sk_mk.egg'),str(result/'source-ast.json')],check=True)
        data=json.loads((result/'source-ast.json').read_text());report=json.loads(baseline_report.read_text())
        code,report=transform(data,report,(source/'source_sk_mk.egg').read_text(),mode)
        (result/'published.egg').write_text(code)
        report['variants']={'published':{'path':str(result/'published.egg'),'sha256':digest(result/'published.egg')}}
        (result/'markers.json').write_text(json.dumps(report,indent=2))
    marked=manager.run('markers',{'source_sk':dest/'source_sk.egg','analysis':analysis,
        'baseline_source':source/'source_sk_mk.egg','baseline_report':baseline_report,'ast_tool':ast},
        {'experiment':'marker-shape','mode':mode},[Path(__file__)],['result/published.egg','result/markers.json','result/source-ast.json'],emit,
        lambda f:json.loads((f/'result/markers.json').read_text()))/'result'
    marked_source=manager.publish(marked/'published.egg','source_sk_mk.egg')
    def emit_ir(folder):
        core=native_core(frontend,marked_source,folder)
        ir=markers_ir(json.loads((marked/'markers.json').read_text()),core,marked_source.read_bytes(),analysis.read_bytes())
        (folder/'core.json').write_text(json.dumps(core));(folder/'marker-program.json').write_text(json.dumps(ir,indent=2))
    ir=manager.run('marker_ir',{'marked_source':marked_source,'markers':marked/'markers.json','analysis':analysis,'frontend':frontend},
        {},[Path(__file__),ROOT/'eggfuse/compiler/marker_ir.py'],['core.json','marker-program.json'],emit_ir,
        lambda f:validate_markers(json.loads((f/'marker-program.json').read_text()),json.loads((f/'core.json').read_text())))
    marker=manager.publish(ir/'marker-program.json','marker-program.json')
    def emit_recipe(folder):(folder/'recipe-program.json').write_text(json.dumps(recipe_ir(json.loads((ir/'core.json').read_text()))))
    recipe=manager.run('recipe_ir',{'core':ir/'core.json'},{'scope':'original_program'},[ROOT/'eggfuse/ir/recipe.py'],
        ['recipe-program.json'],emit_recipe,lambda f:json.loads((f/'recipe-program.json').read_text()))
    recipe=manager.publish(recipe/'recipe-program.json','recipe-program.json')
    compile_target(manager,ir/'core.json',recipe,marker,marked_source,'device',False)
    path,program,new=load_pipeline_artifact(dest)
    assert new['source_sk_sha256']==provenance['source_sk_sha256']
    return path,program

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--pipeline',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--mode',choices=['baseline','remove','if'],required=True);parser.add_argument('--tools',type=Path,default=Path('/Users/mineralsteins/.codex/worktrees/d569/egg_layout/target/debug/examples'))
    args=parser.parse_args();p,_=compile_variant(args.pipeline,args.output,args.mode,args.tools/'eggfuse_frontend',args.tools/'lhs_ast');print(p)
if __name__=='__main__':main()

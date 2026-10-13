#!/usr/bin/env python3
"""Compare frozen old choices and joint pricing on the same native programs.

Both programs use the current emitter and the same cost formula. Frozen choices
are guarded by source, path, layout and implementation hashes. This measures
structural cost and verifies native equality; it is not a time/RSS benchmark.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from tools.cell_spine import compiler as c,markers
from tools.cell_spine.pricing import cost
from tools.cell_spine.terms import text


def fingerprint(value):
    return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':')).encode()).hexdigest()


def describe(plan,retirement,rules,cases):
    metrics={k:sum(cost(r)[k] for r in rules) for k in cost(rules[0])}
    columns=sum(l['columns'] for l in retirement['marker_layouts'].values())
    shapes=plan['costs']['template_nodes']
    source=next(v for v in cases if v['case']=='source')
    return dict(selected=plan['selected'],marker_layouts=plan['marker_layouts'],
        marker_positions={f:dict(position=l['position'],anchor=text(l['anchor']),columns=l['columns'])
                          for f,l in retirement['marker_layouts'].items()},
        template_shapes=shapes,marker_columns=columns,runtime=metrics,
        total=shapes+columns+metrics['search_atoms']+metrics['update_actions'],
        native=dict(status=source['comparison']['status'],validated_cases=len(cases),
            original_enodes=source['original_constructor_rows'],restored_enodes=source['restored_constructor_rows'],
            original_eclasses=source['original_constructor_eclasses'],restored_eclasses=source['restored_constructor_eclasses'],
            original_rules_executed=source['original_rules_executed']))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--skip-build',action='store_true')
    a=p.parse_args();out=a.output.resolve()
    if out.exists() and any(out.iterdir()):raise ValueError('output must be empty')
    out.mkdir(parents=True,exist_ok=True)
    reference=json.loads(Path(__file__).with_name('reference.json').read_text());results=[]
    for case in reference['cases']:
        name=case['fixture'];source=ROOT/case['source'];dest=out/name
        if c.digest(source)!=case['source_sha256']:raise ValueError(f'{name}: reference source changed')
        command=[sys.executable,'-m','tools.cell_spine.compiler','--source',source,'--output',dest,'--storage-cost','structural','--keep-duplicate-spines','--cell-composition','observed']
        if a.skip_build or results:command.append('--skip-build')
        subprocess.run(command,cwd=ROOT,check=True,stdout=subprocess.DEVNULL)
        new=json.loads((dest/'report.json').read_text())
        work=dest/'reference';work.mkdir()
        cs,_=c.load_candidates(Path(new['analysis']),work)
        for candidate in cs:candidate['marker_layouts']=markers.layouts(candidate)
        paths=[dict(id=v['id'],family=v['family'],nodes=v['nodes'],lhs_atoms=v['lhs_atoms']) for v in cs]
        edges=c.transition_options(cs);c.price_options(edges,work)
        plan=case['plan'];selected=[v for v in cs if v['id'] in plan['selected']]
        layouts={str(v['family']):next(l for l in v['marker_layouts'] if l['id'] in plan['marker_layouts']) for v in selected}
        options=[{k:o[k] for k in ('id','body','head','requires')} for e in edges for o in e['options'] if o['id'] in plan['implementations']]
        unique={}
        for o in options:unique.setdefault((tuple(o['body']),tuple(o['head'])),o)
        options=list(unique.values())  # Same exact-rule deduplication as prepare_runtime.
        for label,value in [('paths',paths),('layouts',layouts),('implementations',options)]:
            if fingerprint(value)!=case[label+'_sha256']:raise ValueError(f'{name}: frozen {label} changed')
        data=json.loads((dest/'work/input.json').read_text());sig=dict(data['signatures'])
        env=c.commands(data,{'declaration','rule'})
        for candidate in cs:
            for op,s in candidate['signatures'].items():
                if op not in sig:
                    env.append(f'(constructor {op} ({" ".join(s["input"])}) {s["output"]})');sig[op]=s
        lib,_,_,retirement=c.make_library(cs,plan,edges,env,sig,work)
        manifest=json.loads((dest/'work/validation.json').read_text());manifest['library']=lib
        (work/'validation.json').write_text(json.dumps(manifest))
        subprocess.run([ROOT/'target/release/examples/spine_validate',work/'validation.json',work/'validated.json'],check=True)
        (work/'library.egg').write_text(lib)
        old_plan=dict(plan,costs=dict(template_nodes=len({n for v in selected for n in v['nodes']})))
        old=describe(old_plan,retirement,json.loads((work/'marker-runtime.json').read_text())['rules'],
                     json.loads((work/'validated.json').read_text())['cases'])
        joint=describe(new['plan'],new['retirement'],json.loads((dest/'work/marker-runtime.json').read_text())['rules'],
                       json.loads((dest/'validation.json').read_text())['cases'])
        if joint['total']!=new['plan']['objective'] or joint['total']>old['total']:
            raise ValueError('joint solve disagrees with emitted program or loses to a feasible reference')
        plan=new['plan']
        result=dict(fixture=name,source=case['source'],reference=old,joint=joint,
            solver={k:plan[k] for k in ('status','objective','bound','master','inner_peak','priced_schemes','model_size_scope')},
            cost_audit=new['retirement']['cost_audit']['status'])
        if name=='long_spine_graft':
            result['graft_rule']=next(lines[0] for key,lines in new['retirement']['runtime_rules'].items() if key=='impl_1_0')
        results.append(result)
        print(f'{name}: same formula {old["total"]} -> {joint["total"]}',flush=True)
    report=dict(scope='Structural cost comparison and actual native egglog full-graph equality; not timing or RSS',
        reference_commit=reference['commit'],weights=dict(storage=1,search=1,update=1),cases=results)
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')


if __name__=='__main__':main()

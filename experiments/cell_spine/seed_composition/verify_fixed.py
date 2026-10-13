"""Semantic check with legal fixed layouts; deliberately NOT an optimizer fallback."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from tools.cell_spine import compiler as c,markers,takeover as t
from tools.cell_spine.terms import text


def emit(source,analysis,out):
    work=out/'work';work.mkdir(parents=True)
    data=c.ast(source,work/'input.json');cs,rejected=c.load_candidates(analysis,work)
    for path in cs:path['marker_layouts']=markers.layouts(path)
    reps,_=c.representation_choices(cs);edges=c.transition_options(cs,reps,recompose=True);c.price_options(edges,work)
    chosen={}
    for path in reps:chosen.setdefault(path['family'],path)
    selected={x['id'] for x in chosen.values()}
    plan=dict(selected=sorted(selected),implementations=[next(o['id'] for o in e['options'] if set(o['requires'])<=selected) for e in edges])
    prepared=c.prepare_runtime(cs,plan,edges,work)
    plan['marker_layouts']=[next(l['id'] for l in p['marker_layouts'] if l.get('representation','canonical')=='canonical' and
        set(p['params'].values())<={e['name'] for e in l['terms'] if e['kind']=='var'}) for p in prepared['chosen'].values()]
    sig=dict(data['signatures']);env=c.commands(data,{'declaration','rule'})
    for path in cs:
        for name,s in path['signatures'].items():
            if name not in sig:env.append(f'(constructor {name} ({" ".join(s["input"])}) {s["output"]})');sig[name]=s
    library,chosen,transitions,retirement=c.make_library(cs,plan,edges,env,sig,work,prepared)
    (out/'library.egg').write_text('; Verification-only fixed legal layouts; not a Bake optimum.\n'+library)
    report=dict(status='fixed_legal_layout_semantics_only',not_an_optimum=True,plan=plan,retirement=retirement,
        signatures=data['signatures'],transitions=transitions,rejected=rejected,
        seed_cut_interfaces=sum(e.get('kind')=='certified_seed_cut' for e in edges),
        scope='all certified paths and seed-cut interfaces kept; first representation and canonical full-port markers solely for semantic verification')
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    return report


def verify_add4(source,out,report):
    # The input below is the explicit Add2/Add2 example, not compiler logic.
    ls=report['retirement']['marker_layouts'];small=min(ls,key=lambda f:len(ls[f]['fields']));layout=ls[small]
    if layout.get('representation','canonical')!='canonical' or len(layout['fields'])!=3:raise ValueError('fixture needs a two-input Add2')
    lines=[f'(let {k} (Leaf! "{k}"))' for k in 'abcd']
    for name,x,y in [('left','a','b'),('right','c','d')]:
        lines.append(f'(let {name} (Add!! {x} {y}))');fields=dict(zip(layout['fields'],[t.var(name),t.var(x),t.var(y)]))
        args=[text(markers.canonical(t.replace(e,fields),report['signatures'])) for e in layout['terms']]
        lines.append(f'(Summary-{small}! '+ ' '.join(args)+')')
    lines.append('(let root (Add! left right))');initial='\n'.join(lines)+'\n';(out/'late-input.egg').write_text(initial)
    manifest=dict(constructors=sorted(report['signatures']),library=(out/'library.egg').read_text(),cases=[dict(name='two-compact-add2',
        reference_path=str(source),reference='',compressed=initial+markers.RUNTIME_SCHEDULE,restore='(run-schedule (saturate Export!))')])
    (out/'validation.json').write_text(json.dumps(manifest))
    subprocess.run([ROOT/'target/release/examples/spine_validate',out/'validation.json',out/'validation-result.json'],check=True)
    return json.loads((out/'validation-result.json').read_text())['cases'][0]


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source',type=Path,default=ROOT/'experiments/cell_spine/fixtures/add2_pair_ac.egg')
    p.add_argument('--analysis-dir',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    r=emit(a.source.resolve(),a.analysis_dir.resolve(),out);case=verify_add4(a.source.resolve(),out,r)
    print(json.dumps(dict(status=r['status'],comparison=case['comparison']['status'],original=case['original_constructor_rows'],expanded=case['restored_constructor_rows'])))


if __name__=='__main__':main()

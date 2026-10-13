"""Add a bridge AFTER two small Cells exist. One timing per native variant."""
import argparse
import json
from pathlib import Path
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from tools.bench.ci_cell_spine import measure,read_result
from tools.cell_spine import compiler as c,markers,takeover as t
from tools.cell_spine.terms import text


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--bake-root',type=Path)
    p.add_argument('--variant',action='append',default=[],metavar='NAME=BAKE_DIR',
                   help='compare explicit compiled libraries against the same native input; each must preserve counts')
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--copies',type=int,default=128)
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    if a.copies<1:p.error('positive copies required')
    if a.variant:
        variants={}
        for spec in a.variant:
            name,sep,path=spec.partition('=')
            if not sep or not name or name=='native' or '/' in name or name in variants:p.error('unique NAME=BAKE_DIR required')
            variants[name]=Path(path).resolve()
    elif a.bake_root:
        root=a.bake_root.resolve()
        variants={'observed':root/'add2_pair-rooted/bake','seed-cuts':root/'add2_pair-recomposed/bake'}
    else:p.error('provide --bake-root or --variant')
    first=next(iter(variants.values()))
    data=json.loads((first/'work/input.json').read_text())
    hashes={json.loads((b/'report.json').read_text())['source_sha256'] for b in variants.values()}
    if len(hashes)!=1:p.error('variant sources differ')
    native_env=out/'native-env.egg';native_env.write_text('\n'.join(c.commands(data,{'declaration','rule'})))
    results=[];runner=ROOT/'target/release/examples/native_math_baseline'
    for mode in ['native',*variants]:
        report=None
        if mode!='native':
            bake=variants[mode]
            report=json.loads((bake/'report.json').read_text());ls=report['retirement']['marker_layouts']
            small=min(ls,key=lambda f:len(ls[f]['fields']));layout=ls[small]
            if layout.get('representation','canonical')!='canonical' or len(layout['fields'])!=3:raise ValueError('fixture needs canonical Add2 inputs')
        initial=[];bridges=[]
        for i in range(a.copies):
            for k in 'abcd':initial.append(f'(let {k}_{i} (Leaf{"!" if report else ""} "{i}/{k}"))')
            for name,x,y in [('left','a','b'),('right','c','d')]:
                initial.append(f'(let {name}_{i} (Add{"!!" if report else ""} {x}_{i} {y}_{i}))')
                if report:
                    fields=dict(zip(layout['fields'],[t.var(f'{name}_{i}'),t.var(f'{x}_{i}'),t.var(f'{y}_{i}')]))
                    args=[text(markers.canonical(t.replace(e,fields),data['signatures'])) for e in layout['terms']]
                    initial.append(f'(Summary-{small}! '+ ' '.join(args)+')')
            bridges.append(f'(let root_{i} (Add{"!" if report else ""} left_{i} right_{i}))')
        if mode=='native':initial.append('(run-schedule (saturate algebra))')
        input_file=out/f'{mode}-input.egg';input_file.write_text('\n'.join(initial))
        bridges.append(markers.RUNTIME_SCHEDULE if report else '(run-schedule (saturate algebra))')
        config=dict(environment=str(bake/'library.egg') if report else str(native_env),input=str(input_file),
            run='\n'.join(bridges),constructors=sorted(data['signatures']),counts=False)
        path=out/f'{mode}.json';path.write_text(json.dumps(config))
        elapsed=measure([runner,'bench',path],out/f'{mode}-time');timed=read_result(out/f'{mode}-time')
        config['counts']=True
        if report:config['export']='(run-schedule (saturate Export!))'
        path=out/f'{mode}-counts.json';path.write_text(json.dumps(config))
        measure([runner,'bench-probes',path],out/f'{mode}-counts');diagnostic=read_result(out/f'{mode}-counts');v=diagnostic['verification']
        expected=v['restored'] if mode=='native' else results[0]['restored']
        equivalent_counts=all(v['restored'][k]==expected[k] for k in ('enodes','eclasses'))
        if (a.variant or mode=='seed-cuts') and not equivalent_counts:raise ValueError(f'{mode}: compact bridge lost results')
        globals_={f'{k}_{i}' for i in range(a.copies) for k in ['a','b','c','d','left','right','root']}
        tables=v['native_tables_before_export']
        results.append(dict(mode=mode,equivalent_counts=equivalent_counts,run_seconds=timed['run_seconds'],
            setup_seconds=timed['setup_seconds'],input_seconds=timed['input_seconds'],seconds=timed['seconds'],
            peak_rss_bytes=elapsed['peak_rss_bytes'],physical_rows=v['native_total_rows_before_export'],
            restored=v['restored'],active_markers=v['active_summaries'],retired_markers=v['subsumed_summaries'],
            bake=str(variants[mode]) if report else None,
            compiler_seconds=report.get('compiler_seconds') if report else None,
            interface_selection=report['retirement'].get('interfaces') if report else None,
            rule_count=len(report['retirement']['gate_audit']) if report else sum(c['kind']=='rule' for c in data['commands']),
            structural_cost=report['plan']['costs'] if report else None,
            probes=v.get('run_probe_counters'),
            tables={k:n for k,n in tables.items() if k not in globals_},
            global_binding_rows=sum(n for k,n in tables.items() if k in globals_),
            rule_matches={k:n for k,n in v['rule_matches'].items() if n}))
    (out/'results.json').write_text(json.dumps(dict(copies=a.copies,scope='small native graphs pre-saturated; compact inputs already represent closed Add2; Run includes bridge insertion; one timing each, diagnostics separate; incomplete observed output is not a speed baseline',results=results),indent=2)+'\n')
    print(json.dumps([{k:r[k] for k in ('mode','equivalent_counts','run_seconds','physical_rows','restored')} for r in results],indent=2))


if __name__=='__main__':main()

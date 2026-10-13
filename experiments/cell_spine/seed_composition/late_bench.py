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
    p.add_argument('--bake-root',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--copies',type=int,default=128)
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    if a.copies<1:p.error('positive copies required')
    root=a.bake_root.resolve();data=json.loads((root/'add2_pair-rooted/bake/work/input.json').read_text())
    native_env=out/'native-env.egg';native_env.write_text('\n'.join(c.commands(data,{'declaration','rule'})))
    results=[];runner=ROOT/'target/release/examples/native_math_baseline'
    for mode in ('native','observed','seed-cuts'):
        report=None
        if mode!='native':
            bake=root/('add2_pair-rooted' if mode=='observed' else 'add2_pair-recomposed')/'bake'
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
        measure([runner,'bench-probes',path],out/f'{mode}-counts');v=read_result(out/f'{mode}-counts')['verification']
        expected=v['restored'] if mode=='native' else results[0]['restored']
        equivalent_counts=all(v['restored'][k]==expected[k] for k in ('enodes','eclasses'))
        if mode=='seed-cuts' and not equivalent_counts:raise ValueError('new compact bridge lost results')
        results.append(dict(mode=mode,equivalent_counts=equivalent_counts,run_seconds=timed['run_seconds'],
            setup_seconds=timed['setup_seconds'],input_seconds=timed['input_seconds'],seconds=timed['seconds'],
            peak_rss_bytes=elapsed['peak_rss_bytes'],physical_rows=v['native_total_rows_before_export'],
            restored=v['restored'],active_markers=v['active_summaries'],retired_markers=v['subsumed_summaries'],
            rule_matches={k:n for k,n in v['rule_matches'].items() if n}))
    (out/'results.json').write_text(json.dumps(dict(copies=a.copies,scope='small native graphs pre-saturated; compact inputs already represent closed Add2; Run includes bridge insertion; one timing each, diagnostics separate; incomplete observed output is not a speed baseline',results=results),indent=2)+'\n')
    print(json.dumps([{k:r[k] for k in ('mode','equivalent_counts','run_seconds','physical_rows','restored')} for r in results],indent=2))


if __name__=='__main__':main()

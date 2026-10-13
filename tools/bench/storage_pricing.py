#!/usr/bin/env python3
"""One current-input Bake per cost model, sharing the same captured catalog."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT))
from tools.cell_spine.markers import RUNTIME_SCHEDULE


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--analysis-dir',type=Path)
    args=parser.parse_args();source=args.source.resolve();out=args.output.resolve()
    if out.exists() and any(out.iterdir()):parser.error('output must be empty')
    out.mkdir(parents=True,exist_ok=True)
    analysis=args.analysis_dir.resolve() if args.analysis_dir else None
    result={}
    for mode in ('structural','instances'):
        dest=out/mode
        command=[sys.executable,'-m','tools.cell_spine.compiler','--source',source,'--output',dest,
                 '--skip-build','--storage-cost',mode]
        if analysis is not None:command+=['--analysis-dir',analysis]
        with (out/f'{mode}.log').open('w') as log:
            subprocess.run(command,cwd=ROOT,check=True,stdout=log,stderr=subprocess.STDOUT)
        report=json.loads((dest/'report.json').read_text());analysis=Path(report['analysis'])
        library=(dest/'library.egg').read_text()
        initial=(dest/'runtime.egg').read_text().removeprefix(library).strip()
        if not initial.endswith(RUNTIME_SCHEDULE):raise ValueError('unexpected runtime schedule')
        request=dict(environment=library,input=initial[:-len(RUNTIME_SCHEDULE)],run=RUNTIME_SCHEDULE)
        path=out/f'{mode}-storage.json';path.write_text(json.dumps(request))
        run=subprocess.run([ROOT/'target/release/examples/native_math_baseline','storage',path],
                           check=True,text=True,capture_output=True)
        stored=json.loads(run.stdout.splitlines()[-1])
        result[mode]=dict(storage=stored,selected=report['plan']['selected'],
            marker_layouts=report['plan']['marker_layouts'],objective=report['plan']['objective'],
            compiler_seconds=report['compiler_seconds'],phase_seconds=report['phase_seconds'],
            pricing_evaluations=(report.get('instance_storage') or {}).get('evaluations',0),
            pricing_seconds=(report.get('instance_storage') or {}).get('seconds',0.),
            validation=json.loads((dest/'validation.json').read_text())['cases'][-1]['comparison'])
    (out/'report.json').write_text(json.dumps(result,indent=2)+'\n')
    lines=['# Current-input storage pricing ablation','',
           'Actual native compact execution, one Bake per model; both use the same captured catalog. This is not pristine egglog or an RSS benchmark.',
           '', '| Model | Retained rows | Value slots at input / end | Active / retired markers | Native pricing calls | Pricing seconds |',
           '|---|---:|---:|---:|---:|---:|']
    for mode,r in result.items():
        s=r['storage'];a=s['initial'];b=s['retained']
        lines.append(f'| {mode} | {b["rows"]} | {a["value_slots"]} / {b["value_slots"]} | {b["active_markers"]} / {b["retired_markers"]} | {r["pricing_evaluations"]} | {r["pricing_seconds"]:.6f} |')
    lines+=['','Value slots include output/timestamp/subsume fields. Index bytes, capacity and within-sweep peaks are not priced. Only the first variant captures history; use phase_seconds to compare compilation stages. Full-closure validation is still required for both models.']
    (out/'README.md').write_text('\n'.join(lines)+'\n');print(out/'README.md')


if __name__=='__main__':main()

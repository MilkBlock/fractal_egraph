"""One run per exact pricing mode, sharing the same native Bake catalog."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

ROOT=Path(__file__).resolve().parents[3]
MODES=('enumerate','source-cache','projected')


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source',type=Path,default=ROOT/'experiments/cell_spine/eggcc_if_tiny/source.egg')
    p.add_argument('--analysis-dir',required=True,type=Path)
    p.add_argument('--output',required=True,type=Path)
    p.add_argument('--modes',nargs='+',choices=MODES,default=MODES)
    a=p.parse_args();a.output=a.output.resolve()
    a.output.mkdir(parents=True,exist_ok=False)
    report=dict(schema='cell-layout-pricing/v1',python=platform.python_version(),os=platform.platform(),
        source=str(a.source.resolve()),analysis=str(a.analysis_dir.resolve()),
        scope='actual compiler and native closure validation; reused catalog; one run per mode; native build excluded',results=[])
    for mode in a.modes:
        command=[sys.executable,'-m','tools.cell_spine.compiler','--solver','scip',
            '--source',str(a.source.resolve()),'--analysis-dir',str(a.analysis_dir.resolve()),
            '--output',str(a.output/mode),'--layout-pricing',mode,'--skip-build']
        print(mode+': compile and validate',flush=True);start=time.perf_counter()
        with (a.output/f'{mode}.log').open('w') as log:
            subprocess.run(command,cwd=ROOT,env=dict(os.environ,RAYON_NUM_THREADS='1'),stdout=log,stderr=subprocess.STDOUT,check=True)
        elapsed=time.perf_counter()-start
        r=json.loads((a.output/mode/'report.json').read_text())
        v=json.loads((a.output/mode/'validation.json').read_text())
        assert all(c['comparison']['status']=='equivalent' for c in v['cases'])
        item=dict(mode=mode,process_seconds=elapsed,compiler_seconds=r['compiler_seconds'],
            objective=r['plan']['objective'],model=r['plan']['inner_peak'],pricing=r['pricing_work'],
            source_sha256=r['source_sha256'],native_cases=len(v['cases']),
            paths=r['representation_normalization']['history_paths'],
            physical_rows=v['cases'][-1]['native_total_rows_before_export'],solver=r['solver'],
            artifact_sha256={name:hashlib.sha256((a.output/mode/name).read_bytes()).hexdigest()
                for name in ('library.egg','runtime.egg','restore.egg')})
        if report['results']:
            first=report['results'][0]
            assert math.isclose(item['objective'],first['objective'],rel_tol=1e-9,abs_tol=1e-6)
            assert (item['source_sha256'],item['paths'],item['native_cases'])==(first['source_sha256'],first['paths'],first['native_cases'])
        report['results'].append(item)
        (a.output/'results.json').write_text(json.dumps(report,indent=2)+'\n')
        print(mode+f': {elapsed:.3f}s; objective {item["objective"]}; '+str(item['pricing']),flush=True)
    lines=['# Layout pricing ablation','',report['scope']+'.','',
        '| Mode | Compiler s | Profile s | Full product | Evaluated | Source compilations | LHS normalizations | Objective | Native checks |',
        '|---|---:|---:|---:|---:|---:|---:|---:|---:|']
    for r in report['results']:
        w=r['pricing']
        lines.append(f'| {r["mode"]} | {r["compiler_seconds"]:.3f} | {w["layout_profile_seconds"]:.3f} | {w["original_cartesian_tuples"]} | {w["layout_evaluations"]} | {w["source_preparations"]} | {w["normalization_builds"]} | {r["objective"]} | {r["native_cases"]} |')
    lines+=['','All original layout variables remain available. Equality is checked on the generated native closures, not just counts. The source-only mode includes cached LHS normalization. All modes use the same normalization code; this isolates caching and projection, not the earlier normalizer refactor.','']
    (a.output/'README.md').write_text('\n'.join(lines))


if __name__=='__main__':main()

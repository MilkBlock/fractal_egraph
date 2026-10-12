#!/usr/bin/env python3
"""Diagnose general Cell library reuse; no AC compiler or snapshot fallback."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from tools.cell_spine.frontier import RUNTIME_SCHEDULE
HEADER='''(datatype Term (Leaf String) (Plus Term Term))
(ruleset algebra)
(rewrite (Plus a b) (Plus b a) :ruleset algebra)
(rewrite (Plus (Plus a b) c) (Plus a (Plus b c)) :ruleset algebra)
'''


def tree(n,left,prefix=''):
    leaves=[f'({prefix}Leaf "{i}")' for i in range(n)]
    term=leaves[0] if left else leaves[-1]
    for leaf in leaves[1:] if left else reversed(leaves[:-1]):
        term=f'({prefix}Plus {term} {leaf})' if left else f'({prefix}Plus {leaf} {term})'
    return term


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--skip-build',action='store_true')
    a=p.parse_args();out=a.output.resolve()
    if out.exists() and any(out.iterdir()): raise ValueError('output must be empty')
    out.mkdir(parents=True,exist_ok=True)
    command=[sys.executable,'-m','tools.cell_spine.compiler','--source',ROOT/'experiments/cell_spine/fixtures/assoc.egg','--output',out/'bake']
    if a.skip_build:command.append('--skip-build')
    subprocess.run(command,cwd=ROOT,check=True)
    lib=(out/'bake/library.egg').read_text()
    results=[]
    for n in [3,4,7]:
        for left in [True,False]:
            label=f'{n}-'+('left' if left else 'right')
            case=dict(name=label,reference=HEADER+tree(n,left)+'\n(run-schedule (saturate algebra))',
                      compressed=tree(n,left,'Spine-')+'\n'+RUNTIME_SCHEDULE,
                      restore='(run-schedule (saturate Spine-Export))')
            (out/f'{label}-runtime.egg').write_text(lib+case['compressed']+'\n')
            (out/f'{label}-restore.egg').write_text(lib+case['compressed']+'\n'+case['restore']+'\n')
            manifest=dict(constructors=['Leaf','Plus'],library=lib,dump_states=True,cases=[case])
            path=out/f'{label}.json';path.write_text(json.dumps(manifest))
            r=subprocess.run([ROOT/'target/release/examples/spine_validate',path,out/f'{label}-result.json'],cwd=ROOT,capture_output=True,text=True)
            d=json.loads((out/f'{label}-result.json').read_text())
            if r.returncode:
                if 'full closure mismatch' not in r.stderr:raise RuntimeError(r.stderr)
                result=dict(case=label,status='rejected_incomplete_reuse',original_rows=len(d['expected']['rows']),
                            restored_rows=len(d['actual']['rows']),spine_rows=len(d['spine']['rows']),
                            active_summaries=d['active_summary_tables'])
            else:
                c=d['cases'][0]
                result=dict(case=label,status='equivalent',original_rows=c['original_constructor_rows'],
                            restored_rows=c['restored_constructor_rows'],spine_rows=c['spine_constructor_rows'],
                            original_eclasses=c['original_constructor_eclasses'],restored_eclasses=c['restored_constructor_eclasses'],
                            active_summaries=c['active_summary_tables'],
                            native_total_rows_before_export=c['native_total_rows_before_export'],
                            native_tables_before_export=c['native_tables_before_export'],
                            original_native_total_rows=c['original_native_total_rows'],
                            original_rules_executed=c['original_rules_executed'])
            results.append(result)
    report=dict(native_runtime=True,scope='one frozen three-leaf library on six new input trees; whole-state equality, not a speed/RSS benchmark',
                library_bytes=len(lib.encode()),library_sha256=hashlib.sha256(lib.encode()).hexdigest(),
                storage_scope='All native rows before export, including Read interfaces, Wanted tombstones and Summary bindings; rows have different widths. This is not RSS.',cases=results)
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__=='__main__':main()

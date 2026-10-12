#!/usr/bin/env python3
"""Compare retained table rows using already compiled input-spine examples."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from tools.cell_spine.compiler import cellify,digest
from tools.cell_spine.markers import RUNTIME_SCHEDULE


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--bakes',type=Path,nargs='+',required=True)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
    runner=ROOT/'target/release/examples/native_math_baseline'
    report=dict(scope='actual egglog runtime, same library and input; counts only, no timed speedup claim',
                binary_sha256=digest(runner),cases=[])
    for i,bake in enumerate(a.bakes):
        folder=a.output/str(i);folder.mkdir()
        data=json.loads((bake/'work/input.json').read_text());lib=(bake/'library.egg').read_text()
        source_report=json.loads((bake/'report.json').read_text())
        baseline,restore=cellify(data,lib)
        variants={'raw_ingress':baseline,'input_spine':(bake/'runtime.egg').read_text()}
        item=dict(source=source_report['source'],library_sha256=digest(bake/'library.egg'),
                  input_plan=source_report['input_plan'],variants={})
        for name,program in variants.items():
            body=program.removeprefix(lib).removesuffix(RUNTIME_SCHEDULE+'\n')
            src=folder/f'{name}.egg';src.write_text(body)
            manifest=dict(environment=str((bake/'library.egg').resolve()),input=str(src.resolve()),
                run=RUNTIME_SCHEDULE,export=restore,counts=True,constructors=sorted(data['signatures']))
            path=folder/f'{name}.json';path.write_text(json.dumps(manifest))
            run=subprocess.run([runner,'bench',path],cwd=ROOT,check=True,capture_output=True,text=True)
            v=json.loads(run.stdout.splitlines()[-1])['verification']
            item['variants'][name]={k:v[k] for k in ['raw_spine','canonical_spine','active_summaries',
                'subsumed_summaries','native_total_rows_before_export','restored']}
            item['variants'][name]['tables']={k:n for k,n in v['native_tables_before_export'].items() if n}
        old,new=item['variants']['raw_ingress'],item['variants']['input_spine']
        assert all(old['restored'][k]==new['restored'][k] for k in ('enodes','eclasses'))
        report['cases'].append(item)
        print(Path(item['source']).name,old['native_total_rows_before_export'],'->',new['native_total_rows_before_export'])
    (a.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')


if __name__=='__main__':main()

#!/usr/bin/env python3
"""Run actual native marker programs and record full-state validation/counts."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT))
from tools.cell_spine.terms import text


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--skip-build',action='store_true')
    a=p.parse_args();out=a.output.resolve()
    if out.exists() and any(out.iterdir()):raise ValueError('output must be empty')
    out.mkdir(parents=True,exist_ok=True)
    results=[]
    sources=[(name,ROOT/f'experiments/cell_spine/fixtures/{name}.egg') for name in ('assoc','long_spine_graft','shared_tail','root_alias')]
    sources.append(('eggcc_if',ROOT/'experiments/cell_spine/eggcc_if_tiny/source.egg'))
    for name,source in sources:
        folder=out/name
        command=[sys.executable,'-m','tools.cell_spine.compiler','--source',
            source,'--output',folder]
        if a.skip_build or results:command.append('--skip-build')
        subprocess.run(command,cwd=ROOT,check=True,stdout=subprocess.DEVNULL)
        report=json.loads((folder/'report.json').read_text())
        c=json.loads((folder/'validation.json').read_text())['cases'][-1]
        results.append(dict(fixture=name,source=str(source.relative_to(ROOT)),
            library_bytes=report['library_bytes'],runtime_bytes=report['runtime_bytes'],
            marker_layouts={f:dict(path=l['path'],position=l['position'],anchor=text(l['anchor']),
                stored_fields=[text(t) for t in l['terms']],columns=l['columns'],decoder_lookups=l['lookup_atoms'],payload=l['payload'])
                for f,l in report['retirement']['marker_layouts'].items()},
            gate_audit=report['retirement']['gate_audit'],
            example_graft_rule=(next((report['retirement']['runtime_rules'][t['id']][0] for t in report['transitions']
                if t['parents'] and not t['inherited']),None) if name=='long_spine_graft' else None),
            counts={k:c[k] for k in ('original_constructor_rows','restored_constructor_rows','original_constructor_eclasses',
                'restored_constructor_eclasses','original_native_total_rows','native_total_rows_before_export',
                'native_tables_before_export','active_summary_tables','spine_constructor_rows')},
            status=c['comparison']['status'],original_rules_executed=c['original_rules_executed'],
            solver=dict(status=report['plan']['status'],variables=report['plan']['variables'],constraints=report['plan']['constraints'],
                        objective=report['plan']['objective'],cost_scope=report['plan']['cost_scope'])))
    payload=dict(native_runtime=True,scope='Current-input constructor closures and baked grafts; not a speed or RSS benchmark',cases=results)
    (out/'report.json').write_text(json.dumps(payload,indent=2)+'\n')
    print(json.dumps([dict(fixture=r['fixture'],status=r['status'],**r['counts']) for r in results],indent=2))


if __name__=='__main__':main()

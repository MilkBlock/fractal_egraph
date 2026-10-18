#!/usr/bin/env python3
"""Compact, reproducible text and standard matplotlib plots for completed runs."""
import argparse
import csv
import json
from pathlib import Path

def main():
    p=argparse.ArgumentParser();p.add_argument('report',type=Path);p.add_argument('--output',type=Path,required=True);p.add_argument('--plot',action='store_true');a=p.parse_args()
    r=json.loads(a.report.read_text())
    if not r.get('completed'):raise ValueError('refuse to summarize an incomplete/failed benchmark')
    out=a.output;out.mkdir(parents=True,exist_ok=True)
    summary=dict(schema=r['schema'],platform=r['platform'],baseline_commit=r['build']['baseline_commit'],
        binary_provenance=r['build']['binaries'],rustc=r['build']['rustc'],profile=r['build']['profile'],
        source_hashes=r['build']['sources'],harness_sha256=r['harness_sha256'],library_bytes=r['library_file_bytes'],library=r['library'],threads=r['threads'],notes=r['notes'],cases=[])
    summary['gpu_devices']=sorted({x['timing']['result'].get('stats',{}).get('device_summary',{}).get('device') for c in r['cases'] for x in c['runs']} - {None})
    variants=list(dict.fromkeys(x['name'] for c in r['cases'] for x in c['runs']))
    table=[]
    for c in r['cases']:
        item=dict(kind=c['kind'],instances=c['instances'],lhs_atoms=c['lhs_atoms'],input_sha256=c['input_sha256'],environment_sha256=c['environment_sha256'],graph=c['runs'][0]['validation']['graph'],runs={})
        for x in c['runs']:
            t=x['timing'];z=t['result'];s=z.get('stats',{})
            row=dict(core_seconds=z['core_seconds'],setup_seconds=z['setup_seconds'],input_seconds=z['input_seconds'],run_seconds=z['run_seconds'],
                process_wall_seconds=t['process_wall_seconds'],sampled_peak_process_tree_rss_bytes=t['sampled_peak_process_tree_rss_bytes'],rss_samples=t['rss_samples'],
                validation=dict(exact_equal=x['validation']['graph']['exact_equal'],primitive_values_exact=x['validation']['graph']['primitive_values_exact'],strict_gpu_check=x['validation']['strict_gpu_check'],seconds=x['validation']['seconds'],stats=x['validation']['stats']),native_matches=z.get('native_matches'),consumer_bindings=s.get('accepted_bindings'),native_sets=s.get('native_sets'),
                phase_seconds=s.get('phase_seconds'),install_breakdown=s.get('install_breakdown'),marker_work=s.get('marker_work'),solver=s.get('physical_layout'),device=s.get('device_summary'),
                table_runtime=s.get('table_runtime'),table_counters={k:v for k,v in s.items() if k.startswith('table_') and k!='table_runtime'})
            item['runs'][x['name']]=row
            table.append(dict(case=c['kind'],instances=c['instances'],variant=x['name'],core_seconds=row['core_seconds'],run_seconds=row['run_seconds'],process_wall_seconds=row['process_wall_seconds']))
        summary['cases'].append(item)
    (out/'results.json').write_text(json.dumps(summary,indent=2)+'\n')
    with (out/'timings.csv').open('w') as f:
        w=csv.DictWriter(f,fieldnames=list(table[0]),lineterminator='\n');w.writeheader();w.writerows(table)
    lines=['# Release benchmark results','',
        'Core time = setup + input + all driver rounds, including marker maintenance, planning, transfer and commit. Each variant is one fresh process. Verification runs separately; graph/value/visibility equality is checked against pristine.',
        '', '| Case | Instances | Expr nodes / classes | ' + ' | '.join(v+' core (s)' for v in variants)+' |',
        '|---|---:|---:|'+'---:|'*len(variants)]
    for c in summary['cases']:
        z=c['runs'];g=c['graph']['sorts']['Expr']
        lines.append(f'| {c["kind"]} | {c["instances"]} | {g["enodes"]} / {g["eclasses"]} | '+ ' | '.join(f'{z[k]["core_seconds"]:.6f}' if k in z else '—' for k in variants)+' |')
    lines += ['', '## Largest cases: measured run phases', '', '| Case / variant | Run (s) | Admission (s) | CPU routing (s) | Installation (s) | Dispatch (s) | GPU device (s) |', '|---|---:|---:|---:|---:|---:|---:|']
    for kind in sorted({c['kind'] for c in summary['cases']}):
        c=max((c for c in summary['cases'] if c['kind']==kind),key=lambda c:c['instances'])
        for name,z in c['runs'].items():
            s=z['phase_seconds']
            if not s:continue
            lines.append(f'| {kind} / {c["instances"]} / {name} | {z["run_seconds"]:.6f} | '+ ' | '.join(f'{s[k]:.6f}' for k in ['admission_seconds','routing_seconds','install_seconds','dispatch_seconds','device_seconds'])+' |')
    lines += ['', 'GPU device time is a subset of dispatch; do not add these columns. Static table shader compilation is in setup. These are controlled workloads using an analyzed Single/Get Cell, not a complete eggcc pass.', '', f'Library files: {r["library_file_bytes"]:,} bytes. RSS measurements are sampled sums of currently live processes and can miss peaks or count shared pages twice. They are not egraph compression ratios.', '', 'See `results.json` for exact graph sizes, primitive verification, phase breakdowns, binary hashes and source provenance.']
    (out/'RESULTS.md').write_text('\n'.join(lines)+'\n')
    if a.plot:
        import matplotlib
        matplotlib.use('Agg')
        import matplotlib.pyplot as plt
        fig,axes=plt.subplots(1,2,figsize=(11,4.7))
        colors={'pristine':'#202020','fork':'#888888','marker-paired':'#176baf','marker-solver':'#c76516','device-tables':'#209266','cartesian-gram':'#944078','marker-gram':'#ab8888'}
        for ax,kind in zip(axes,['local','sparse-cycle']):
            cs=sorted((c for c in summary['cases'] if c['kind']==kind),key=lambda c:c['instances'])
            for variant,color in colors.items():
                present=[c for c in cs if variant in c['runs']]
                if present:ax.plot([c['instances'] for c in present],[c['runs'][variant]['core_seconds'] for c in present],marker='o',label=variant,color=color)
            ax.set(xscale='log',yscale='log',xlabel='Instances',ylabel='Setup + input + run (seconds)',title=kind)
            ax.grid(True,which='both',alpha=.2);ax.legend(fontsize=8)
        fig.suptitle('Same-result release benchmark — one fresh process per point')
        fig.text(.5,.015,'OS / Metal caches were not flushed. Correctness replay is excluded from these timings.',ha='center',fontsize=9)
        fig.tight_layout(rect=[0,.04,1,.94]);fig.savefig(out/'timings.png',dpi=160);fig.savefig(out/'timings.svg');plt.close(fig)

if __name__=='__main__':main()

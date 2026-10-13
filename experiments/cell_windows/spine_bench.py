"""Root-prefix vs middle-history Cells through real Bake and native runtime.

Reuse the CI measurement harness. Audit actual multi-marker consumers rather
than assuming that every newly discovered window participates in a merge.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT))
from tools.bench.ci_cell_spine import benchmark_case,machine,render as runtime_report,read_result
from tools.cell_spine import compiler as c,names
from tools.cell_spine.terms import text,walk

CASES={'math_tiny':'experiments/cell_spine/math_tiny/source.egg',
       'eggcc_if':'experiments/cell_spine/eggcc_if_tiny/source.egg',
       'boundary':'experiments/cell_spine/fixtures/input_spine_boundary.egg'}


def merge_rules(rules,window_only):
    result=[]
    for rule in rules:
        reads={text(e):names.family(e['head']) for fact in rule['body']
            for root in (fact['args'] if fact['kind']=='eq' else [fact['expr']]) for e in walk(root)
            if e['kind']=='call' and names.is_summary(e['head'])}
        writes={names.family(e['head']) for action in rule['head'] if action['kind']!='change'
            for root in ([action['expr']] if action['kind'] in ('expr','let') else action['args']) for e in walk(root)
            if e['kind']=='call' and names.is_summary(e['head'])}
        if len(reads)>=2 and writes:
            result.append(dict(rule=rule['name'],source_markers=len(reads),source_families=list(reads.values()),
                targets=sorted(writes),uses_window_only_family=bool(window_only.intersection(reads.values()))))
    return result


def audit(folder):
    bake=folder/'bake';r=json.loads((bake/'report.json').read_text())
    q=json.loads((bake/'analysis/analysis.json').read_text())['saturated_rule_composition']
    g=q['cell_egraph'];window_ids={w['unit'] for w in g['windows'] if w['new_entry']}
    rooted_families={p['family'] for p in r['paths'] if p['id'] not in window_ids}
    window_only={p['family'] for p in r['paths']}-rooted_families
    edges=json.loads((bake/'transitions.json').read_text())
    chosen=[o for e in edges for o in e['options'] if o['id'] in r['plan']['implementations']]
    rules=json.loads((bake/'work/marker-runtime.json').read_text())['rules']
    mergers=merge_rules(rules,window_only)
    matching={}
    for mode in ('raw_ingress','input_spine'):
        counts=read_result(folder/f'{mode}-counts')['verification']['rule_matches']
        matching[mode]=dict(all_merge_matches=sum(counts.get(x['rule'],0) for x in mergers),
            window_only_merge_matches=sum(counts.get(x['rule'],0) for x in mergers if x['uses_window_only_family']),
            by_rule={x['rule']:counts.get(x['rule'],0) for x in mergers})
    result=dict(registered_entries=len(q['jobs']),states=q['counts'],certified_cell_kinds=len(g['reuse_value']['cells']),
        history_windows=len(g['windows']),new_window_entries=len(window_ids),
        window_only_families=sorted(window_only),supported_paths=len(r['paths']),
        rejected_paths=r.get('rejected',[]),representations=r['representation_normalization']['representations'],
        observed_edges=len(edges),observed_edges_with_new_window_parent=sum(bool(window_ids.intersection(e['observed_parents'])) for e in edges),
        chosen_multi_parent_implementations=sum(len(o['parents'])>=2 for o in chosen),
        chosen_implementations_using_window_representation=sum(any(p['path'] in window_ids for p in o['parents']) for o in chosen),
        emitted_rules=len(rules),multi_marker_merge_rules=mergers,merge_matches=matching,
        objective=r['plan']['objective'],solver_model=r['plan']['master'],
        all_original_paths_validated=len(json.loads((bake/'validation.json').read_text())['cases']),
        artifact_sha256={n:c.digest(bake/n) for n in ('library.egg','runtime.egg','restore.egg')})
    (folder/'cell-audit.json').write_text(json.dumps(result,indent=2)+'\n')
    return result


def render(report):
    lines=['# Root-prefix vs middle-history Cell Bake','',
        'Actual current egglog kernel, not pristine. One fresh Bake per variant, one timing per runtime mode. Runtime counts are collected in separate processes. No timing averages. All original layout choices remain available.',
        '', '| Input | Discovery | Entries | Certified Cell kinds | Supported paths / representations | Bake s | Runtime rules | Multi-marker merge rules | Window-parent edges |',
        '|---|---|---:|---:|---:|---:|---:|---:|---:|']
    for key,row in report['cases'].items():
        a=row['cell_audit'];lines.append(f'| {key} | {row["discovery"]} | {a["registered_entries"]} | {a["certified_cell_kinds"]} | {a["supported_paths"]} / {a["representations"]} | {row["bake"]["compiler_seconds"]:.3f} | {a["emitted_rules"]} | {len(a["multi_marker_merge_rules"])} | {a["observed_edges_with_new_window_parent"]} |')
    lines+=['','| Input / discovery | Mode | Physical rows | Active / retired markers | Expanded nodes / classes | Run ms | Install + input + run + export ms | RSS MiB | Merge-rule matches / involving new window-only families | Library KiB |',
        '|---|---|---:|---:|---:|---:|---:|---:|---:|---:|']
    for key,row in report['cases'].items():
        for mode in ('native','raw_ingress','input_spine','input_spine_export'):
            s=row['modes'][mode]['size'];t=row['modes'][mode]['timing'];a=row['cell_audit']
            m=a['merge_matches'].get(mode.removesuffix('_export'))
            matched=f'{m["all_merge_matches"]} / {m["window_only_merge_matches"]}' if m else '—'
            library='—' if mode=='native' else f'{row["bytes"]["library.egg"]/1024:.2f}'
            lines.append(f'| {key} | {mode} | {s["physical_rows"]} | {s["active_markers"]} / {s["retired_markers"]} | {s["restored_enodes"]} / {s["restored_eclasses"]} | {1000*t["run_seconds"]:.3f} | {1000*t["seconds"]:.3f} | {t["peak_rss_bytes"]/2**20:.2f} | {matched} | {library} |')
    lines+=['','Rows include auxiliary and retired rows. Expanded nodes/classes are measured after actual export, never inferred by multiplying templates. Logical merge-rule matches are not new Cell insertions. Sub-millisecond differences from single runs are not reliable speed evidence.',
        '', 'The windows switch adds each apply and each complete immediate producer-consumer neighborhood to the exact closure catalog. It does not recursively register every new window in the discovery frontier. A zero window-parent-edge count therefore cannot be presented as a successful experiment in arbitrary small-Cell recombination.',
        '', 'Raw ingress measures the generated matcher from ordinary forked facts. Input spine also uses solver-selected direct initialization; it may avoid merge matches by constructing the chosen Cell immediately. The report exposes both paths.',
        '', 'Seed/history cases receive native full-closure validation. The replicated runtime workload is checked by measured expanded node/class counts against native execution, not by full graph isomorphism.',
        '', 'The two catalogs may impose different transition obligations. Their solver objective values are diagnostic, not a like-for-like runtime quality score. Full per-table and phase results are in `phases.md` / `report.json`.','']
    return '\n'.join(lines)


def measurements(report):
    """Small text evidence; full table/rule detail stays in the reproducible output."""
    result={k:v for k,v in report.items() if k!='cases'};result['cases']={}
    for name,row in report['cases'].items():
        a=row['cell_audit']
        summary={k:v for k,v in a.items() if k not in ('multi_marker_merge_rules','merge_matches')}
        summary['multi_marker_merge_rule_count']=len(a['multi_marker_merge_rules'])
        summary['merge_matches']={m:{k:v for k,v in vs.items() if k!='by_rule'} for m,vs in a['merge_matches'].items()}
        result['cases'][name]=dict(discovery=row['discovery'],source=row['source'],source_sha256=row['source_sha256'],
            bake=row['bake'],bytes=row['bytes'],cell_audit=summary,
            modes={mode:dict(timing=stats['timing'],size={k:v for k,v in stats['size'].items() if k!='tables'}) for mode,stats in row['modes'].items()})
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--cases',nargs='+',choices=CASES,default=['math_tiny','eggcc_if'])
    p.add_argument('--copies',type=int,default=128)
    a=p.parse_args()
    if a.copies<1:p.error('copies must be positive')
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    os.environ.update(RAYON_NUM_THREADS='1',EGG_LAYOUT_GRAPHS='0',EGG_LAYOUT_EAGER_SVG='0',
        EGG_LAYOUT_ANALYSIS_THREADS='1',EGG_LAYOUT_QUERY_THREADS='1',EGG_LAYOUT_RIPEN_OBSERVE_MS='50',
        EGG_LAYOUT_RIPEN_OBSERVE_FACTS='200000',EGG_LAYOUT_RIPEN_SWEEP_STEPS='4',EGG_LAYOUT_SOLVER='scip')
    for k in ('EGG_LAYOUT_FULL_TRACE','EGG_LAYOUT_RIPEN_HISTORY','EGG_LAYOUT_PROFILE','EGGLOG_JOIN_WORK_RULES','EGG_LAYOUT_COMPOSITION_DISCOVERY'):os.environ.pop(k,None)
    report=dict(schema='cell-window-spine-bench/v1',copies=a.copies,environment=machine(),
        commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),
        binaries={str(p.relative_to(ROOT)):c.digest(p) for p in [ROOT/'target/release/egg_layout',ROOT/'target/release/examples/native_math_baseline']},cases={})
    for name in a.cases:
        expected=None
        for mode,enabled in [('rooted','0'),('windows','1')]:
            os.environ['EGG_LAYOUT_CELL_WINDOWS']=enabled;key=f'{name}-{mode}'
            row=benchmark_case(key,ROOT/CASES[name],out,a.copies)
            row['discovery']=mode;row['cell_audit']=audit(out/key)
            size=row['modes']['input_spine']['size'];logical=(size['restored_enodes'],size['restored_eclasses'])
            if expected is not None and logical!=expected:raise ValueError('logical workload changed across discovery modes')
            expected=logical;report['cases'][key]=row
            (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
            (out/'measurements.json').write_text(json.dumps(measurements(report),indent=2)+'\n')
            (out/'README.md').write_text(render(report))
            (out/'phases.md').write_text(runtime_report(report))
            print(key+': audited',flush=True)
    print(out/'README.md')


if __name__=='__main__':main()

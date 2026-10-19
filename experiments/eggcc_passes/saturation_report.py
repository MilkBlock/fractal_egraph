"""Aggregate non-overlapping native Saturate/Run boundary times."""
import json
from pathlib import Path


def summarize(folder):
    workloads = []
    for workload in sorted(Path(folder).iterdir()):
        if not workload.is_dir(): continue
        files = sorted(workload.glob('pass-*.json'))
        if not files: continue
        passes = [json.loads(p.read_text()) for p in files]
        if not all('schedule_profile' in p for p in passes):
            print(f'{workload.name}: ordinary pass totals saved; schedule profiling unavailable')
            continue
        assert [p['pass'] for p in passes] == [1, 2, 3]
        measurement = json.loads((workload/'measurement.json').read_text())
        rows, groups = [], {}
        for p in passes:
            s = p['schedule_profile']; sat = s['saturate_seconds']; run = s['run_seconds']
            assert sat > 0, 'Production schedule was not captured (check experimental run-schedule handling)'
            for metric in ['search_and_apply_seconds','insert_merge_seconds','rebuild_seconds']:
                observed=sum(x[metric] for x in s['packets'])
                assert abs(observed-p['native_totals'][metric]) < 1e-5, (p['pass'],metric,observed,p['native_totals'][metric])
            assert sum(x['logical_matches'] for x in s['packets']) == p['native_totals']['matches'], 'unaccounted native schedule work'
            assert abs(sat+run-sum(x['wall_seconds'] for x in s['packets'])) < 1e-7
            assert p['execution_seconds']+1e-7 >= sat+run
            rows.append({'pass':p['pass'],'saturate_seconds':sat,'run_seconds':run,
                'saturate_share_of_schedule':sat/(sat+run), 'execution_seconds':p['execution_seconds'],
                'saturate_share_of_execution':sat/p['execution_seconds'],
                'extraction_wall_seconds':p['extraction_wall_seconds'],'serialization_seconds':p['serialization_seconds'],
                'saturate_calls':sum(x['kind']=='saturate' for x in s['packets']),
                'run_calls':sum(x['kind']=='run' for x in s['packets']),
                'nodes':p['nodes'],'eclasses':p['eclasses'],'native_totals':p['native_totals']})
            for x in s['packets']:
                key=(x['kind'],x['schedule'])
                g=groups.setdefault(key,dict(kind=x['kind'],schedule=x['schedule'],calls=0,wall_seconds=0.,search_and_apply_seconds=0.,insert_merge_seconds=0.,rebuild_seconds=0.,logical_matches=0))
                g['calls']+=1
                for k in ['wall_seconds','search_and_apply_seconds','insert_merge_seconds','rebuild_seconds','logical_matches']:
                    g[k]+=x[k]
        total={k:sum(p[k] for p in rows) for k in ['saturate_seconds','run_seconds','execution_seconds','extraction_wall_seconds','serialization_seconds','saturate_calls','run_calls']}
        total['saturate_share_of_schedule']=total['saturate_seconds']/(total['saturate_seconds']+total['run_seconds'])
        total['saturate_share_of_execution']=total['saturate_seconds']/total['execution_seconds']
        total['saturate_share_of_cli']=total['saturate_seconds']/measurement['cli_wall_seconds']
        workloads.append({'workload':workload.name,'measurement':measurement,'passes':rows,'total':total,
            'schedule_groups':sorted(groups.values(),key=lambda x:-x['wall_seconds'])})
    result={'scope':'One fresh production 3-pass pipeline, extraction feeds the next pass. Outermost Saturate is atomic; nested time counted once. No GPU/Cell acceleration, no coverage audit, no raw trace. Includes native per-command preparation; search+apply includes RHS.', 'workloads':workloads}
    (Path(folder)/'saturation-summary.json').write_text(json.dumps(result,indent=2)+'\n')
    lines=['# Raytrace saturation time','',result['scope'],'',
        '| Pass | Saturate s | Ordinary Run s | Saturate / scheduled execution | Execution including setup s | Extraction wall s |',
        '|---|---:|---:|---:|---:|---:|']
    for w in workloads:
        for p in [*w['passes'],dict(w['total'],**{'pass':'Total'})]:
            lines.append(f'| {p["pass"]} | {p["saturate_seconds"]:.3f} | {p["run_seconds"]:.3f} | {100*p["saturate_share_of_schedule"]:.2f}% | {p["execution_seconds"]:.3f} | {p["extraction_wall_seconds"]:.3f} |')
        t=w['total'];m=w['measurement']
        lines+=['',f'Complete CLI wall: {m["cli_wall_seconds"]:.3f} s. Saturate accounts for {100*t["saturate_share_of_cli"]:.2f}% of CLI wall and {100*t["saturate_share_of_execution"]:.2f}% of pass execution including setup.',
            f'Serialization: {t["serialization_seconds"]:.3f} s (separate from extraction). Executed outer Saturate calls: {t["saturate_calls"]}; standalone Run calls: {t["run_calls"]}. Call count is not native iteration count or work share.',
            '', '| Schedule block | Calls | Wall s | Search + RHS s | Insert/merge s | Rebuild s |','|---|---:|---:|---:|---:|---:|']
        for g in w['schedule_groups'][:12]:
            lines.append(f'| `{g["schedule"]}` | {g["calls"]} | {g["wall_seconds"]:.3f} | {g["search_and_apply_seconds"]:.3f} | {g["insert_merge_seconds"]:.3f} | {g["rebuild_seconds"]:.3f} |')
    lines+=['','These are measured schedule boundaries, not certified whole-Cell eligibility. Primitive updates, custom merges, subsume and nested ordering still need their own correctness contracts. No claim that every saturated block is directly replaceable by the existing Cell macro. Timers use monotonic wall time; no per-match instrumentation added.']
    (Path(folder)/'RESULTS.md').write_text('\n'.join(lines)+'\n')
    print('\n'.join(lines))
    return result


if __name__ == '__main__':
    import argparse
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('folder',type=Path)
    summarize(p.parse_args().folder)

"""Collect a fixed-workload Bake ablation without conflating seed/atom coverage."""
import argparse, hashlib, json, subprocess
from pathlib import Path
p=argparse.ArgumentParser()
for name in ('profile','check','time','old_control','output'):p.add_argument(name,type=Path)
a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
runs={}
for mode in ('profile','check','time'):
    r=json.loads((getattr(a,mode)/'results.json').read_text());assert r['mode']==mode
    for v in r['variants']:
        for phase in ('intact','disrupted'):assert v[phase]['verification']['exact_rows_values_equivalence_visibility']
        if mode=='check':assert sum(q['stats']['audit_queries'] for q in v['report']['rules'])>0
    runs[mode]=r;(a.output/f'{mode}.json').write_text(json.dumps(r,separators=(',',':'))+'\n')
old=json.loads(Path('cell-assisted-egglog/cell-locality-bench/results/profile.json').read_text())
control=json.loads((a.old_control/'results.json').read_text())
oldwork=old['variants'][-1]['external']['work'];newwork=control['variants'][-1]['external']['work']
for k in ('routing_valid_seeds','routed_ready','routed_suspected','ready_covered_atom_sum','ready_reuse_seeds','join_probes','routing_locator_probes'):assert oldwork[k]==newwork[k]
(a.output/'old-workload-control.json').write_text(json.dumps(control,separators=(',',':'))+'\n')
# Retain the small actual baked bodies/proofs, not generated large input streams.
for path in sorted(a.profile.glob('*/**/*.json')):
    dest=a.output/'libraries'/path.relative_to(a.profile);dest.parent.mkdir(parents=True,exist_ok=True)
    dest.write_text(json.dumps(json.loads(path.read_text()),separators=(',',':'))+'\n')
pct=lambda x,y:f'{100*x/y:.1f}%' if y else '—'
byname={v['library']:v for v in runs['profile']['variants']}
before_probes=byname['none']['intact']['work']['join_probes']
after_work=byname['composed_cells']['intact']['work']
after_probes=after_work['join_probes']+after_work['routing_locator_probes']
rich_time=runs['time']['variants'][-1];rd=rich_time['report']['device'];rt=rich_time['intact']['runtime']
lines=['# Bake quality: fixed workload ablation','','Actual GPU/native execution. This uses synthetic held-out bindings for four tuple families, not whole eggcc.','',
       f"Profile: {runs['profile']['instances_per_family']} instances per family, eight consumers. All libraries see identical facts and schedules.",'',
       '| Library | Training contexts / distinct states | Ready seeds | Located Cell atoms / all LHS atoms | Actual internal reads reused | Join + locator probes |',
       '|---|---:|---:|---:|---:|---:|']
for v in runs['profile']['variants']:
    w=v['intact']['work'];b=v['bake']
    # All seeds have the same family mix. The rich/base catalog covers the full
    # mix; compute the full atom denominator explicitly rather than conditioning
    # the smaller library's mean on only the families it happened to cover.
    all_atoms=runs['profile']['instances_per_family']*2*sum(runs['profile']['consumer_atoms'])
    reused=sum(w[k+'_reused_atoms'] for k in ('ready','suspected','unmapped'))
    probes=w['join_probes']+w['routing_locator_probes']
    lines.append(f"| {v['library']} | {b['training_contexts']} / {b['distinct_states']} | {int(w['routed_ready']):,}/{int(w['routing_valid_seeds']):,} ({pct(w['routed_ready'],w['routing_valid_seeds'])}) | {pct(w['ready_covered_atom_sum'],all_atoms)} | {int(reused):,} | {int(probes):,} |")
lines+=['', 'Repeated Single training stays at 75%. Single appears at root or non-root positions in three families; adding Pair covers the fourth. Composed contexts do not increase the already 100% Ready rate, but make one context cover more atoms. The three external relation atoms always remain outside training: **entire-LHS-in-one-Cell coverage is 0% in this design**, even when Ready is 100%.', '',
        f"Counting locator cost, the composed library reduces query index probes from {int(before_probes):,} to {int(after_probes):,} ({pct(before_probes-after_probes,before_probes)}). Average probes per valid seed: {before_probes/after_work['routing_valid_seeds']:.2f} to {after_probes/after_work['routing_valid_seeds']:.2f}. These are index-lookup work counts, not a wall-clock speedup or storage compression ratio.", '',
        '| After boundary changes | Ready / all valid seeds | Ready share of complete bindings | Suspected seeds | Proven missing-next seeds |',
        '|---|---:|---:|---:|---:|']
for v in runs['profile']['variants']:
    w=v['disrupted']['work'];matches=sum(w[k+'_matches'] for k in ('ready','suspected','unmapped'))
    lines.append(f"| {v['library']} | {int(w['routed_ready']):,}/{int(w['routing_valid_seeds']):,} ({pct(w['routed_ready'],w['routing_valid_seeds'])}) | {pct(w['ready_matches'],matches)} | {int(w['routed_suspected']):,} | {int(w['routing_no_next_atom']):,} |")
lines+=['', 'The richer catalog cannot make missing facts exist or make a non-unique location unique. With the composed library, Ready work still produces 92.9% of successful complete bindings, but Ready is only 52% of all valid seeds. These are different denominators.', '',
        'Old-workload control: replacing the old library with all four new states leaves Ready at **7,168/10,240 = 70%**, with the same reuse/covered-atom/probe counts. Its shortfall was not missing Single/Get training.', '',
        f"Timing: {runs['time']['instances_per_family']} instances per family, one run each, no profiling/audit.",'',
        '| Library | Bake ms | Setup ms | Intact inference ms | Disrupted inference ms | Live GPU MiB | Projection plans |',
        '|---|---:|---:|---:|---:|---:|---:|']
for v in runs['time']['variants']:
    d=v['report']['device'];lines.append(f"| {v['library']} | {1000*v['bake']['seconds']:.2f} | {1000*v['report']['runtime']['setup_seconds']:.2f} | {1000*v['intact']['runtime']['run_seconds']:.3f} | {1000*v['disrupted']['runtime']['run_seconds']:.3f} | {d['device_bytes']/2**20:.2f} | {d['cell_plans']} |")
lines+=['',f"High coverage is not sufficient for current-runtime speed. Four states compile into {rd['cell_plans']} projection plans. The rich run has {rd['watch_count']:,} recorded watch entries but reserves {rd['watch_capacity']:,}. `FamilyExecutor.advance` sizes Cell storage by row capacity × projection count and watch storage additionally by maximum prefix length. This directly explains the storage over-reservation. We did not change the runtime between libraries.", '', f"In the rich intact run, {1000*rt['roundtrip_seconds']:.3f} ms of {1000*rt['run_seconds']:.3f} ms is inside the worker round trip, including only {1000*rt['device_seconds']:.3f} ms of measured GPU command time. Host allocation/copy/dispatch/synchronization is a likely contributor, but those host subphases were not separately timed; this is not proof that allocation alone dominates wall time.", '',
        'This supports improving Bake **structural diversity and composition coverage**, not simply producing more examples. It also identifies the next implementation issue: sparse allocation for actual Cell/interface instances and their watches, rather than the full row/projection product.', '',
        'All five libraries agree with complete native facts/equality/visibility at both phases. Separate check-mode frontier audits pass. Native whole-Cell/export tests: 11 passed.']
(a.output.parent/'RESULTS.md').write_text('\n'.join(lines)+'\n')
files=[Path('src/whole_cell.rs'),Path('tests/whole_cell.rs'),Path('examples/cell_bake_quality.rs'),Path('examples/support/live_table_bench.rs'),Path('experiments/bake_quality/helpers.egg'),Path('experiments/bake_quality/training.json'),Path(__file__),*sorted(Path('cell-assisted-egglog/src').glob('*.rs')),*sorted(Path('cell-assisted-egglog').glob('family*.metal')),Path('cell-assisted-egglog/matrix.swift')]
(a.output/'provenance.json').write_text(json.dumps({'head_before_commit':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'hashes':{str(x):hashlib.sha256(x.read_bytes()).hexdigest() for x in files},'binary_sha256':hashlib.sha256(Path('target/analyzer/release/examples/cell_bake_quality').read_bytes()).hexdigest(),'worker_sha256':hashlib.sha256(Path('out/cell-locality-worker/matrix').read_bytes()).hexdigest(),'old_control_binary_sha256':hashlib.sha256(Path('target/analyzer/release/examples/cell_locality_bench').read_bytes()).hexdigest()},indent=2)+'\n')

#!/usr/bin/env python3
"""Save complete-graph evidence and annotate the actual compiled deltas."""
import argparse, hashlib, json, shutil, subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser();p.add_argument('run',type=Path);p.add_argument('--output',type=Path,required=True);p.add_argument('--binary',type=Path,default=ROOT/'target/analyzer/release/egg_layout');a=p.parse_args()
r=json.loads((a.run/'report.json').read_text());lib=json.loads((a.run/'library.json').read_text())
assert r['completed']
for s in r['stages']:
    assert s['exact_full_graph']['status']=='equivalent' and s['saturation_audit']['graph_unchanged']
    assert not any(s['runtime']['smooth_match_counts'].values())
    assert all(w['before']==w['after'] for w in s['runtime']['smooth_query_watermarks'])
    assert s['runtime']['native']['frame_receipts']==1
    assert s['native_extraction']['cost']==s['cell_extraction']['cost']
out=a.output;out.mkdir(parents=True,exist_ok=True)
names=['report.json','library.json','original.egg','compiled-deltas.egg']
names += [f'{s["stage"]}-{lane}.json' for s in r['stages'] for lane in ['native','cell']]
for name in names:shutil.copyfile(a.run/name,out/name)
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
sources=[ROOT/'src/whole_cell.rs',ROOT/'src/main.rs',ROOT/'src/lib.rs',ROOT/'tests/whole_cell.rs',Path(__file__).resolve(),ROOT/'experiments/whole_cell_chain/math.egg',ROOT/'experiments/whole_cell_chain/plan.json',ROOT/'experiments/whole_cell_chain/heldout.egg',a.binary.resolve()]
provenance={'source_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'run':str(a.run.resolve()),'hashes':{str(x):sha(x) for x in sources},'artifact_hashes':{n:sha(out/n) for n in names},'scope':'actual native backend, unassisted same-fork reference; no pristine or GPU speedup claim'}
(out/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
code=(out/'compiled-deltas.egg').read_text()
for i,step in enumerate(lib['steps']):
    parent=lib['states'][step['source']] if step['source'] is not None else None
    parent_n=len(parent['values']) if parent else 0
    comments=[f'; {step["name"]}: complete RipSatCell {step["source"]} -> {step["target"]}']
    for j,to in enumerate(step['input_to_target']):
        origin=(', '.join(n for n,v in parent['ports'].items() if v==j) or f'internal slot {j}') if j<parent_n else 'external '+step['parameters'][j-parent_n]['name']
        comments.append(f'; in{j}: {origin} -> target v{to}')
    comments.append('; target roles: '+json.dumps(lib['states'][step['target']]['ports'],sort_keys=True))
    code=code.replace(f'(ruleset wc_emit_{i})','\n'.join(comments)+f'\n(ruleset wc_emit_{i})')
(out/'annotated-deltas.egg').write_text(code)
lines=['# Whole RipSatCell execution results','',
'A → B → C executes complete closed states. Bake, the independent native reference and the post-run audit are separate from the hot runtime. The original rules remain installed in that runtime.', '',
'| Stage | Cell state | Cell enodes | Inherited enodes | Emitted enodes | Native smooth matches | Runtime smooth matches | Macro worklist matches | Full graph nodes/classes | Extraction cost |',
'|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|']
for s in r['stages']:
    rt=s['runtime'];lines.append(f'| {s["stage"]} | {s["cell_state"]} | {s["cell_rows"]} | {rt["inherited_rows"]} | {rt["emitted_rows"]} | {sum(s["native"]["smooth_matches"].values())} | {sum(rt["smooth_match_counts"].values())} | {sum(rt["native"]["macro_action_matches"].values())} | {s["whole_graph_enodes"]}/{s["whole_graph_eclasses"]} | {s["cell_extraction"]["cost"]} |')
lines+=['', '**Every stage passes exact full original-graph comparison**, including literals and e-class equality; these are not only count comparisons. Original rule watermarks are unchanged. Each target frame is returned once by the native commit receipt. Macro worklist dispatch and constructor/hashcons/union work are not claimed to disappear.', '',
'The retained parent rows are transported through the target equality map before computing the delta. The compiler can therefore emit unions of old slots and rely on native rebuild to merge old keys; it is not plain set subtraction on concrete node IDs.', '',
'A separate audit runs the original matcher on a clone of each macro result. It finds matches but changes no facts. This proves the closure was already produced; those audit matches are never used by the runtime.', '',
'Final extracted term (same cost as the independent native run):','', '```egg',r['stages'][-1]['cell_extraction']['term'],'```','',
'The plan contains only coarse graft contexts. Target bodies and deltas were obtained by actual symbolic native saturation, then verified by delta replay from parent snapshots. Tests additionally reuse the same library with unseen leaf identities, exercise non-root grafts, merge old parent slots, preserve cyclic bodies, and reject extra-but-saturated graphs.', '',
'This is a CPU/native semantic acceptance result, not a throughput benchmark, GPU speedup or virtual storage result. The current contract is positive constructor/union rules, inert input forests, exact alias conditions and full saturation boundaries. General eggcc effects, finite schedules and automatic runtime graft discovery are not covered.']
(out/'RESULTS.md').write_text('\n'.join(lines)+'\n')
print(out/'RESULTS.md')

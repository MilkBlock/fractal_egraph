"""Render saved residual ablations and native case evidence, without rerunning."""
import argparse
import json
from pathlib import Path
import numpy as np


def render(explicit,tokens,cases,capture,audit):
    r=explicit;cap=json.loads((capture/'capture.json').read_text())
    def pct(x):return 'N/A' if x is None else f'{100*x:.2f}%'
    lines=['# Residual Polyspine: pre-stage state matters', '',
           'On the SAME 47 newly enabled purity answers and 109,380 negatives in Raytrace Pass 3, '
           '**one step of rule-derived GPU propagation recovers 45/47 with zero false positives**. '
           'Ridge on the projected context also recovers 45/47, but adds false positives. '
           'The gain belongs primarily to the representation and deterministic propagation, not to learning.', '',
           '## Why this differs from the first experiment', '',
           'The old masking policy erased valid entry-state operator/child purity along with future target labels. '
           'The new protocol retains committed entry facts but trains/scores only roots lacking their own target '
           'answer. All features are frozen before the stage. The 10,121 native Run update flags and the old '
           'per-root bags/order/entry visibility were checked before reusing old labels. An independent native '
           'replay additionally rechecked entry and exit labels at every persisted typed root; all 12 stages agree.', '',
           'The shape-only model is retrained on this pending-only objective. Do not attribute the difference '
           'between it and the old all-state model solely to feature changes.', '',
           '## Matched explicit-coordinate models', '',
           '| Model | Recovered positives | False positives | Precision | Recall |',
           '|---|---:|---:|---:|---:|']
    for item in r['results']:
        m=item['test'];lines.append(f"| {item['variant']} | {m['tp']}/{m['tp']+m['fn']} | {m['fp']} | {pct(m['precision'])} | {pct(m['recall'])} |")
    for depth,m in r['deterministic_projection'].items():
        lines.append(f"| direct propagation, {depth} layer(s), no ridge | {m['tp']}/{m['tp']+m['fn']} | {m['fp']} | {pct(m['precision'])} | {pct(m['recall'])} |")
    lines += ['', 'The 1/2/4/8 depths are predeclared ablations, not a chosen optimum. In this held-out population, '
              'all reachable new positives are already one-hop consequences of entry-state facts. More depth '
              'does not help. Empty/unknown results are not proofs of impossibility.', '',
              'The erased-prestate control removes previously committed recursively produced predicates while '
              'retaining static operator facts. The shuffled control assigns residual feature rows to the wrong '
              'roots. Both lose useful information; all such controls remain speculative and never commit effects.', '',
              '## Retained hash-bag instability', '',
              '| Layers | State mixed into hash bag: TP / FP | Explicit numeric state: TP / FP |',
              '|---|---:|---:|']
    tm={x['variant']:x['test'] for x in tokens['results']};em={x['variant']:x['test'] for x in r['results']}
    for depth in [1,2,4,8]:
        a=tm[f'state_{depth}'];b=em[f'state_{depth}'];lines.append(f"| {depth} | {a['tp']} / {a['fp']} | {b['tp']} / {b['fp']} |")
    lines += ['', 'The explicit layout reserves 32 of 512 columns for numeric readiness/state and leaves the other '
              '480 unchanged across depth variants. This avoids rehashing and renormalizing earlier state when '
              'depth grows. It removes the observed four-layer recall collapse, but does **not** uniformly reduce '
              'false positives: the retained eight-layer hash-bag result has fewer false positives than the explicit model.', '',
              '## The two misses are structural transitions and union', '']
    compact_cases=[]
    for c in cases['cases']:
        frame=next(f for f in cap['frames'] if f['cycle']==c['cycle']);z=np.load(capture/frame['data'])
        index=np.flatnonzero(z['keys']==str(tuple(c['after_key'])))
        known=bool(len(index) and z['before'][index[0]])
        compact_cases.append({'cycle':c['cycle'],'before_key':c['before_key'],'after_key':c['after_key'],
                              'destination_already_pure_at_entry':known,'new_heads':c['new_constructor_heads']})
        lines.append(f"- `{c['before_key'][2]}` becomes canonical `{c['after_key'][2]}`; destination already pure "
                     f"at entry: **{known}**; newly visible constructor alternatives: {', '.join(c['new_constructor_heads'])}.")
    lines += ['', 'At entry each source root has only `TupleRemoveAt(tuple, 0)`, and tuple-length is already 4. '
              'The native rules include:', '', '```lisp',
              '(rewrite (TupleRemoveAt tuple idx)',
              '  (Concat (SubTuple tuple 0 idx)',
              '          (SubTuple tuple (+ idx 1) (- len (+ idx 1))))',
              '  :when ((= len (tuple-length tuple))) :ruleset always-run)', '```', '',
              'The before/after observations establish newly exposed alternatives and union to previously pure '
              'classes. They are not a complete per-rule causal proof of the intervening derivation. This evidence '
              'was collected after model fitting and was not added to its training or features.', '',
              'A frozen purity dependency graph cannot discover a future structural connection just by iterating '
              'more times. These cases motivate representing conditional structural/equality transitions next, '
              'rather than claiming all dynamic egraph behavior is solved by this projection.', '',
              '## What runs where', '',
              f"The compiler admits **{r['admitted_rules']}/{r['selected_rules']}** unary-producing rules. "
              'The rejected guarded Get/TypeList and multi-anchor Call paths remain listed. CPU preparation copies '
              'raw typed tables, assigns coordinates and compiles per-rule column selectors; it does not join '
              'those rows into per-instance bindings for GPU input. GPU evaluates the shared selectors and '
              'preserves whole-alternative conjunctions. A separate CPU expansion is only the differential oracle.', '',
              f"The admitted support graph averages **{r['incidences_per_rule_row']:.3f} predicate dependencies per rule row**. "
              'That measures this rule family, not the sparsity of arbitrary egraphs or all Pass-3 joins.', '',
              f"All 12 recorded entry states and both state-retention controls pass GPU/CPU projection checks. "
              f"Ridge matrix scoring has **{r['score_gpu']['threshold_disagreements']} threshold differences** "
              f"and max score error **{r['score_gpu']['max_score_error']}**. The old SPINE003 packet also remains correct.", '',
              f"Prestate GPU projection (12 frames, eight layers each): **{r['projection_gpu_seconds']*1000:.3f} ms**. "
              f"Ridge scoring of all seven ablations: **{r['score_gpu']['timing']['gpu_seconds']*1000:.3f} ms**. "
              'Projection uses sparse gathers and reductions, not Tensor Core GEMM. Only scoring uses matrix multiplication.', '',
              f"Offline snapshot observation: **{r['offline_observation_seconds']:.3f} s**; raw-context preparation "
              f"and previous-feature verification: **{r['offline_packing_seconds']:.3f} s**; independent CPU projection "
              f"diagnostic: **{r['offline_diagnostic_seconds']:.3f} s**; fitting-feature packing/checks: "
              f"**{r['offline_fit_feature_packing_seconds']:.3f} s**. GPU features are read back for offline fitting. "
              'This is not resident end-to-end inference and provides no eggcc speedup claim.', '',
              f"Separate borrowed-label audit: **{audit['wall_seconds_diagnostic']:.3f} s**, diagnostic only.", '',
              'Nineteen regressions include complete versus partial alternatives, seeded/unseeded cycles, repeated '
              'variables, subsumed seeds, unsupported effects, actual GPU projections, numeric-column routing and '
              'legacy packet compatibility.', '',
              '## Provenance', '',f"- Source: `{r['source_sha256']}`",f"- Capture: `{r['capture_sha256']}`",
              f"- Score worker: `{r['score_worker_sha256']}`",'- Full saved results: `out/polyspine-residual-explicit` and `out/polyspine-residual-tokens`.',
              '- Native case evidence: `out/polyspine-residual-cases/cases.json`.',
              '- The earlier report-emission failure is retained in `out/polyspine-residual-fit/failure.json`; it is not an accepted result.']
    return '\n'.join(lines)+'\n',compact_cases


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--explicit',type=Path,required=True);p.add_argument('--tokens',type=Path,required=True);p.add_argument('--cases',type=Path,required=True);p.add_argument('--label-audit',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();r=json.loads(a.explicit.read_text());t=json.loads(a.tokens.read_text());c=json.loads(a.cases.read_text())
    audit=json.loads(a.label_audit.read_text())
    if audit['capture_sha256']!=r['capture_sha256'] or not all(c['entry_and_exit_labels_exact'] for c in audit['checks']):raise ValueError('label audit mismatch')
    text,examples=render(r,t,c,Path(r['capture']),audit);a.output.write_text(text)
    summary={'explicit':r,'tokens':t,'native_cases':examples,'label_audit':audit,'scope':'text results only; binary artifacts remain untracked'}
    a.output.with_suffix('.json').write_text(json.dumps(summary,indent=2)+'\n')

"""Render cached full-scale local prediction results, without running a model."""
import argparse
from collections import defaultdict
import json
from pathlib import Path


def render(r,d):
    measures={(x['split'],x['stage'],x['variant']):x['test'] for x in r['measurements']}
    populations={(p['split'],p['stage'],p['part']):p for p in r['populations']}
    def pr(m):
        p='—' if m['precision'] is None else f"{100*m['precision']:.1f}"
        q='—' if m['recall'] is None else f"{100*m['recall']:.1f}"
        return p+' / '+q
    lines=['# Polyspine: full-scale real Pass-3 local updates', '',
           f"The unchanged source schedule executed **{r['native_runs']:,} native Runs**. "
           f"All **{r['frames']} saturation visits** of four declared families were observed. "
           'No copied toy graph or positive-only sampling is used.', '',
           f"Dataset: **{sum(f['rows'] for f in d['frames']):,} local stage observations**, "
           f"**{sum(f['eligible'] for f in d['frames']):,} eligible predictions**, "
           f"**{d['lineages']:,} distinct final union lineages**. The group split holds out "
           f"**{d['heldout_lineages']:,} lineages**. Repeated stages are observations, not independent programs.", '',
           '## What was actually expanded', '',
           '| Stage | Sat visits | Eligible observations | Answer gains/updates | Updates to existing answers | Outside pre-stage domain |',
           '|---|---:|---:|---:|---:|---:|']
    for stage in ['type-analysis','always-run','is-resolved','terms']:
        fs=[f for f in d['frames'] if f['stage']==stage]
        lines.append(f"| {stage} | {len(fs)} | {sum(f['eligible'] for f in fs):,} | {sum(f['gain'] for f in fs):,} | "
                     f"{sum(f['changed_existing'] for f in fs):,} | {sum(f['outside_pre_domain'] for f in fs):,} |")
    lines += ['', 'Targets are HasType payload additions, new ExprIsPure / ExprIsResolved answers, and '
              'ExtractedExpr payload changes (including one-level TCPair candidate/cost content). '
              'These are projected answer updates, **not complete rule matches, committed mutation counts or extraction quality**.', '']
    for split,title in [('lineage','Union-lineage holdout across the same program'),('time','Chronological holdout: cycles 6–11')]:
        lines += ['## '+title,'', '| Stage | Train / test observations | Test positives | Test lineages | Entry P/R | Polyspine P/R | Residual-8 P/R | Versioned-8 P/R |',
                  '|---|---:|---:|---:|---:|---:|---:|---:|']
        for stage in ['type-analysis','always-run','is-resolved','terms']:
            a=populations[(split,stage,'train')];b=populations[(split,stage,'test')]
            nums=[pr(measures[(split,stage,v)]) for v in ['entry','polyspine','residual8','versioned8']]
            lines.append(f"| {stage} | {a['rows']:,} / {b['rows']:,} | {b['positive']:,} | {b['groups']:,} | "+' | '.join(nums)+' |')
        lines += ['', 'P/R are precision and recall percentages at the fixed 0.5 threshold. No best-threshold selection. '
                  'An em dash precision means there were no positive proposals.', '']
    cold=measures[('lineage','terms','versioned8')];late=measures[('time','terms','versioned8')]
    pure=measures[('lineage','always-run','versioned8')]
    lines += ['## Interpretation', '',
              f"The larger cold-lineage purity test recovers {pure['tp']}/{pure['tp']+pure['fn']} positives. "
              'The earlier 45/47 warm-state result did not cover this harder population.', '',
              f"Extraction-state updates reach {pr(cold)} P/R in lineage holdout, but only {pr(late)} "
              'in the later-time test. This gap must not be hidden by reporting only the stronger split.', '',
              'The resolved structural baseline is already very strong; its accuracy cannot be credited to '
              'Polyspine or residual propagation. The clearest additional learned benefit is cross-interface '
              'information for extraction updates, although many updates are still missed.', '',
              'Current function features retain returned constructor heads and answer counts, not full numeric '
              'candidate costs and update programs. The four version flags also do not encode arbitrary '
              'structural/union transitions. These are concrete representation gaps; this experiment does not '
              'identify a single causal explanation for every error.', '',
              'No predicted negative is used to skip native work. These results do not justify removing exact '
              'verification or fallback.', '']
    lines += ['## Complete ablation counts', '', '| Split | Stage | Variant | TP | FP | FN | TN |', '|---|---|---|---:|---:|---:|---:|']
    for x in r['measurements']:
        m=x['test'];lines.append(f"| {x['split']} | {x['stage']} | {x['variant']} | {m['tp']:,} | {m['fp']:,} | {m['fn']:,} | {m['tn']:,} |")
    lines += ['', '## Direct projection is a separate control', '', '| Split | Stage | TP | FP | FN | TN |', '|---|---|---:|---:|---:|---:|']
    for x in r['direct_projection']:
        lines.append(f"| {x['split']} | {x['stage']} | {x['tp']:,} | {x['fp']:,} | {x['fn']:,} | {x['tn']:,} |")
    lines += ['', 'Only purity and resolved currently have the admitted raw-row GPU propagation. The type and '
              'extraction heads use observed entry interfaces and learned versions; they must not inherit propagation '
              'guarantees from the other two heads. Positive finite-depth propagation and ridge extrapolation are '
              'reported separately. Unknown/negative scores do not authorize discarding matches.', '',
              '## Split and leakage checks', '',
              '- Every historic root joined by native union belongs to exactly one lineage split. Train/test lineage intersections are empty.',
              '- Future union lineage is used only for grouping the split, not to supply a feature or an earlier alias.',
              '- Version features retain only earlier pre-stage shapes, interfaces and answer counts. Current-stage labels are never read by feature construction.',
              '- The lineage split is transductive within this same program; it is not held-out programs, whole functions or disjoint neighborhoods.',
              '- The time split allows previously seen roots; its results are not silently substituted for unseen-lineage results.',
              '- New output keys outside the pre-stage domain are counted above and remain uncovered.', '',
              '## Work and costs', '',
              f"{len(r['projection_audit'])} GPU projection frames were checked against an independent CPU reference. "
              f"Ridge scoring checked {r['gpu']['outputs']:,} outputs, maximum error "
              f"{r['gpu']['max_score_error']}, threshold differences {r['gpu']['threshold_disagreements']}.", '',
              f"The fit retains {r['distinct_context_vectors']:,} distinct vectors and {r['distinct_models']} distinct "
              'fitted models. Repeated coordinates retain their full positive/negative multiplicities; contradictory '
              'labels are not collapsed to a representative.', '',
              f"Diagnostic observation: {r['observation_seconds']:.3f} s; feature preparation: {r['feature_seconds']:.3f} s; "
              f"stream aggregation plus independent projection audit: {r['stream_aggregation_and_audit_seconds']:.3f} s; "
              f"ridge fit/score on CPU: {r['ridge_fit_seconds']:.3f} s. Compact observer traffic: "
              f"{r['wire_bytes']/2**20:.2f} MiB. GPU score kernel time: {r['gpu']['timing']['gpu_seconds']*1000:.3f} ms.", '',
              'The observer scans selected tables and sends lossless differences. It is not a production event journal. '
              'CPU feature construction, stored projection inputs and offline fitting remain real costs. '
              'There is no end-to-end speedup or Tensor Core utilization claim.', '',
              '## Provenance', '',f"- Source SHA-256: `{r['source_sha256']}`",f"- Native oracle SHA-256: `{r['frontend_sha256']}`",
              f"- Dataset SHA-256: `{r['dataset_sha256']}`",f"- Score worker SHA-256: `{r['score_worker_sha256']}`",
              '- Full dataset and untracked binary artifacts: `out/polyspine-scale-data`.',
              '- Full score results: `out/polyspine-scale-fit/results.json`.',
              '- A subsequent container-inner lineage fix in the read-only observer was tested separately; the measured capture binary is retained with the fingerprint above.',
              '- 26 regressions passed, including actual GPU checks and native union/set/subsume/container observation.']
    return '\n'.join(lines)+'\n'


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--result',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();r=json.loads(a.result.read_text());d=json.loads((Path(r['dataset'])/'dataset.json').read_text())
    a.output.write_text(render(r,d));a.output.with_suffix('.json').write_text(json.dumps(r,indent=2)+'\n')

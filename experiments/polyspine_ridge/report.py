"""Render saved Polyspine measurements; never replays native/GPU work."""
import argparse
import json
from pathlib import Path


def percent(value):
    return 'N/A' if value is None else f'{100*value:.2f}%'


def render(result, dataset):
    by_variant={v['variant']:{h['stage']:h for h in v['heads']} for v in result['variants']}
    baseline=by_variant['structure']['always-run'];poly=by_variant['polyspine']['always-run']
    pending_total=poly['pending']['tp']+poly['pending']['fn']
    pairs={(p['before'],p['stage']):p for p in result['paired']}
    def pair_text(name):
        p=pairs[(name,'always-run')]
        return f"fixes {p['fixed']:,} held-out decisions and breaks {p['broken']:,} ({p['fixed']-p['broken']:,} net)"
    improved=(poly['test']['fp']+poly['test']['fn'])<(baseline['test']['fp']+baseline['test']['fn'])
    less_new=poly['pending']['tp']<baseline['pending']['tp']
    headline=('A small gain on purity classification, but worse recall on newly enabled answers.'
              if improved and less_new else 'Matched feature ablation; see separate total and newly enabled populations below.')
    lines = ['# Polyspine ridge: Raytrace Pass 3', '',
             f'**{headline}**', '',
             'This is an offline, causally sampled representation test. It does not accelerate the production runtime.', '',
             f"Native replay checked all **{result['native_runs']:,} Run update flags** against the saved verified trace. "
             f"We sampled **{result['frames']} Saturate boundaries** across 12 outer cycles; "
             'cycles 0–5 train and 6–11 test. Every variant uses the same rows, ridge penalty 1, '
             '512 coordinates, FP16 weights and threshold 0.5.', '',
             f"Total observations: **{sum(f['rows'] for f in dataset['frames']):,}** across four targets. "
             'These are repeated Expr/stage observations, not unique Cells or native rule matches.', '',
             '## always-run / ExprIsPure', '',
             '| Coordinates | Precision | Recall | False positives | Missed positives | Raw ridge Brier |',
             '|---|---:|---:|---:|---:|---:|']
    for variant in result['variants']:
        h=next(h for h in variant['heads'] if h['stage']=='always-run');m=h['test']
        lines.append(f"| {variant['variant']} | {percent(m['precision'])} | {percent(m['recall'])} | {m['fp']:,} | {m['fn']:,} | {m['brier']:.5f} |")
    lines += ['', f'**Unknown at stage entry:** {pending_total} positive answers are newly enabled in the held-out purity population. '
              'The remaining candidates are negative. Do not use total accuracy or already-present answers to hide this small, difficult subset.', '',
              '| Coordinates | Newly enabled answers recovered | False proposals | Precision | Recall |',
              '|---|---:|---:|---:|---:|']
    for variant in result['variants']:
        h=next(h for h in variant['heads'] if h['stage']=='always-run');m=h['pending']
        lines.append(f"| {variant['variant']} | {m['tp']}/{m['tp']+m['fn']} | {m['fp']:,} | {percent(m['precision'])} | {percent(m['recall'])} |")
    lines += ['', f"Relative to local interfaces, correct Polyspine wiring {pair_text('interfaces')}. "
              f"Relative to shuffled wiring, it {pair_text('shuffled_seams')}. "
              'These are paired decisions on the same rows, not independent programs or a significance claim.', '',
              '## Other stage families', '',
              '| Stage / target | Structure precision | Structure recall | Polyspine change |',
              '|---|---:|---:|---|']
    for h in result['variants'][0]['heads']:
        if h['stage']=='always-run':continue
        lines.append(f"| {h['stage']} / {h['target']} | {percent(h['test']['precision'])} | {percent(h['test']['recall'])} | None: no admitted Expr-to-interface seam |")
    lines += ['', 'The current read-set and target masking produce useful cross-stage features only in always-run '
              '(mostly HasType/HasArgType interfaces supplied by earlier stages). The others are explicit negative '
              'controls, not evidence that Polyspine works or fails for their complete rule systems. In particular, '
              'the terms task needs candidate/parameter and recurrent-state features beyond this one-edge interface view.', '',
              'Stage labels alone change no decision here: the model already has a head per target, and the sampled '
              'previous-stage labels are nearly fixed. This does not establish that schedule context is generally useless.', '',
              '## What this result diagnoses', '',
              '- Connected interfaces contain a little information: the correctly wired model beats the same bags shuffled among roots.',
              f"- Newly enabled purity recovery changes from {baseline['pending']['tp']}/{pending_total} to "
              f"{poly['pending']['tp']}/{pending_total}. This must be weighed separately from old answers.",
              '- The Polyspine token signatures still mix positive and negative answers: even a test-oracle majority lookup '
              f"would make {poly['signature_irreducible_errors']:,} purity errors "
              f"(versus {baseline['signature_irreducible_errors']:,} for structure). This is a descriptive information-loss bound, "
              'not a trained model or an independent benefit metric; random signature splitting can also reduce it.',
              '- We encode observed one-edge overlaps and possible producing rulesets, not complete multi-stage derivation '
              'programs or witnessed per-row rule histories. Missing recursive context, correlated alternatives and parameter '
              'updates remain plausible causes; this experiment does not separately identify their contributions.',
              '- Do not compare these percentages directly with the older final-snapshot new-binding split: labels and holdout are different.', '',
              '## Computation and limits', '']
    gpu=result['gpu']
    lines += [f"The existing Apple matrix worker evaluated all five variants in **{gpu['timing']['gpu_seconds']*1000:.3f} ms** "
              f"device time ({gpu['timing']['host_submit_wait_seconds']*1000:.3f} ms encode/submit/wait). "
              f"It emitted {gpu['outputs']:,} scores, including unused heads. FP16/FP32 score comparison: "
              f"maximum error **{gpu['max_score_error']}**, **{gpu['threshold_disagreements']}** threshold disagreements.", '',
              f"Offline CPU feature extraction: **{result['offline_extraction_seconds']:.3f} s**; "
              f"packing: **{result['offline_packing_seconds']:.3f} s**; "
              f"snapshot observation: **{result['diagnostic_snapshot_seconds']:.3f} s**. These costs are real. "
              'No end-to-end acceleration or GPU utilization claim is made.', '',
              'The worker is reused unchanged; generic “Analyze-derived” captions inside its raw gpu.json describe '
              'its historical packet format, not this dataset\'s provenance. Here the evidence is the native pre-stage '
              'snapshot and typed rules. Final predicate truth is never used to construct features.', '',
              '## Provenance', '',
              f"- Source SHA-256: `{result['source_sha256']}`",
              f"- Native oracle SHA-256: `{result['frontend_sha256']}`",
              f"- Dataset SHA-256: `{result['dataset_sha256']}`",
              f"- Worker SHA-256: `{result['worker_sha256']}`",
              f"- Full local result: `{result['dataset']}/../polyspine-pass3-ridge/results.json`", '',
              'Nine regressions cover connected versus disconnected contexts, argument position, alpha renaming, '
              'target/private-sort masking, visibility, union label mapping, nested scheduling, ridge aggregation and conflicting labels.']
    return '\n'.join(lines)+'\n'


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--result',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();r=json.loads(a.result.read_text());d=json.loads((Path(r['dataset'])/'dataset.json').read_text())
    a.output.write_text(render(r,d))

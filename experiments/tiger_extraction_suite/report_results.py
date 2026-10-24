#!/usr/bin/env python3
"""Generate figures and report tables from validated suite packets; never run benchmarks."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import statistics

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt


def gm(xs):
    xs=list(xs)
    return math.exp(statistics.mean(math.log(x) for x in xs)) if xs else None


def fmt(x,digits=3):
    return f'{x:.{digits}f}' if x is not None else '---'


def tex(s):
    return ''.join({'_':r'\_','%':r'\%','&':r'\&','#':r'\#','$':r'\$'}.get(c,c) for c in str(s))


def ratio(a,b):return a/b if b else None


def main():
    p=argparse.ArgumentParser();p.add_argument('--input',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--preliminary',type=Path)
    args=p.parse_args();root=args.input;out=args.output;out.mkdir(parents=True,exist_ok=True)
    packet=json.loads((root/'summary.json').read_text())
    assert packet['manifest'].get('timing_protocol')=='blocking-wait-with-separate-watchdog/v2'
    assert packet['completed']==packet['attempted']==packet['inventory'], 'Incomplete suite'
    rows=packet['rows'];cases={p.parent.name:json.loads(p.read_text()) for p in sorted((root/'cases').glob('*/result.json'))}
    valid=[r for r in rows if r['frozen_checked']];pipeline=[r for r in rows if r['pipeline_speedup'] is not None]
    graphs=[g for c in cases.values() for g in c['graphs']]
    native_sum=sum(r['native_extraction_seconds'] for r in valid);bag_sum=sum(r['bag_extraction_seconds'] for r in valid)
    old_phase=sum(r['native_estimate_seconds'] for r in valid);new_phase=sum(r['bag_estimate_seconds'] for r in valid)
    native_pipeline=sum(r['native_pipeline_seconds'] for r in pipeline);bag_pipeline=sum(r['bag_pipeline_seconds'] for r in pipeline)
    inside={mode:sum(cases[r['name']]['pipeline'][mode]['eggcc_extraction_time_seconds'] for r in pipeline) for mode in ['native','bag']}
    stats=dict(inventory=packet['inventory'],validated_programs=len(valid),validated_graphs=sum(g['quality_passed'] for g in graphs),
               pipeline_validated=len(pipeline),cost_pick_entries=sum(s['classes'] for g in graphs for s in g['runs']['oracle']['stats'] if 'classes' in s),
               extraction_geomean=gm(r['extraction_speedup'] for r in valid),extraction_aggregate=ratio(native_sum,bag_sum),
               native_extraction_seconds=native_sum,bag_extraction_seconds=bag_sum,
               estimate_geomean=gm(r['estimate_speedup'] for r in valid),estimate_aggregate=ratio(old_phase,new_phase),
               native_estimate_seconds=old_phase,bag_estimate_seconds=new_phase,
               pipeline_geomean=gm(r['pipeline_speedup'] for r in pipeline),pipeline_aggregate=ratio(native_pipeline,bag_pipeline),
               native_pipeline_seconds=native_pipeline,bag_pipeline_seconds=bag_pipeline,
               pipeline_extraction_seconds=inside,rss_geomean=gm(r['rss_ratio'] for r in valid if r['rss_ratio']),
               extraction_wins=sum(r['extraction_speedup']>1 for r in valid),extraction_slower=sum(r['extraction_speedup']<1 for r in valid),
               phase_wins=sum(r['estimate_speedup']>1 for r in valid),
               maximum_arity=max(g['graph']['max_arity'] for g in graphs),
               summary_sha256=hashlib.sha256((root/'summary.json').read_bytes()).hexdigest())
    repeat_path = root / 'correctness-repeat.json'
    repeat = None
    if args.preliminary:
        prior=json.loads((args.preliminary/'summary.json').read_text())
        previous={p.parent.name:json.loads(p.read_text()) for p in (args.preliminary/'cases').glob('*/result.json')}
        repeat = dict(manifest=prior['manifest'], cases=previous)
    elif repeat_path.exists():
        # Compact receipts retain only hashes and provenance, never invalidated times.
        repeat = json.loads(repeat_path.read_text())
    if repeat is not None:
        assert all(repeat['manifest'][k]==packet['manifest'][k] for k in ['binary','native','candidate','inventory'])
        previous=repeat['cases']
        assert set(previous)==set(cases), 'Repeat receipt inventory differs'
        checked=[]
        for name,c in cases.items():
            old=previous[name]
            checked.append(all(c['pipeline'][m]['stdout_sha256']==old['pipeline'][m]['stdout_sha256']
                               and c['pipeline'][m]['graphs']==old['pipeline'][m]['graphs'] for m in ['native','bag']))
        stats['additional_repeat_equal_programs']=sum(checked)
        stats['preliminary_external_timings_used']=False
    groups=[]
    labels={'bril':'Bril','polybench':'PolyBench','raytrace':'Raytrace','fenwick':'Fenwick'}
    for name in ['bril','polybench','raytrace','fenwick']:
        group=[r for r in valid if r['suite']==name]
        groups.append(dict(suite=name,n=len(group),phase=gm(r['estimate_speedup'] for r in group),
                           extractor=gm(r['extraction_speedup'] for r in group),
                           aggregate=ratio(sum(r['native_extraction_seconds'] for r in group),sum(r['bag_extraction_seconds'] for r in group)),
                           pipeline=gm(r['pipeline_speedup'] for r in group if r['pipeline_speedup']),
                           rss=gm(r['rss_ratio'] for r in group if r['rss_ratio'])))
    stats['groups']=groups
    (out/'report-stats.json').write_text(json.dumps(stats,indent=2)+'\n')

    plt.rcParams.update({'font.family':'DejaVu Sans','font.size':9,'axes.spines.top':False,'axes.spines.right':False,
                         'axes.titleweight':'semibold','pdf.fonttype':42,'savefig.bbox':'tight'})
    figures=out/'figures';figures.mkdir(exist_ok=True)
    fig,ax=plt.subplots(figsize=(6.5,3.0))
    for key,label,color in [('estimate_speedup','Cost estimation','#205D85'),('extraction_speedup','Whole Tiger','#29744A'),('pipeline_speedup','Pipeline with capture','#87612E')]:
        xs=sorted(r[key] for r in rows if r.get(key) is not None)
        ax.step(xs,[(i+1)/len(xs) for i in range(len(xs))],where='post',label=f'{label} (n={len(xs)})',color=color,lw=1.8)
    ax.axvline(1,color='#777777',ls='--',lw=1)
    ax.set_xscale('log');ax.set_xlabel('Original time / shared-bag time  (greater than 1 is faster)')
    ax.set_ylabel('Fraction of programs');ax.set_ylim(0,1.03);ax.grid(axis='y',alpha=.18)
    ax.legend(frameon=False,loc='lower right');fig.tight_layout();fig.savefig(figures/'speedup-distribution.pdf');fig.savefig(figures/'speedup-distribution.png',dpi=180);plt.close(fig)

    fig,ax=plt.subplots(figsize=(6.5,3.5));colors={'bril':'#205D85','polybench':'#29744A','raytrace':'#BD7436','fenwick':'#9D426A'}
    all_times=[r[k] for r in valid for k in ['native_extraction_seconds','bag_extraction_seconds']]
    lo,hi=min(all_times)*.7,max(all_times)*1.4
    ax.plot([lo,hi],[lo,hi],color='#888888',ls='--',lw=1,label='Equal time')
    for suite in labels:
        group=[r for r in valid if r['suite']==suite]
        ax.scatter([r['native_extraction_seconds'] for r in group],[r['bag_extraction_seconds'] for r in group],
                   label=labels[suite],color=colors[suite],s=22 if len(group)>1 else 50,alpha=.8)
    for r in valid:
        if r['suite']=='raytrace':ax.annotate('raytrace',(r['native_extraction_seconds'],r['bag_extraction_seconds']),xytext=(-50,-18),textcoords='offset points',fontsize=8)
    ax.set_xscale('log');ax.set_yscale('log');ax.set_xlim(lo,hi);ax.set_ylim(lo,hi)
    ax.set_xlabel('Original Tiger time, three pass snapshots (s)');ax.set_ylabel('Shared-bag Tiger time (s)')
    ax.grid(alpha=.15,which='major');ax.legend(frameon=False,fontsize=8);fig.tight_layout()
    fig.savefig(figures/'paired-extraction.pdf');fig.savefig(figures/'paired-extraction.png',dpi=180);plt.close(fig)

    abstract=(f'All {len(valid)} programs and {stats["validated_graphs"]} pass snapshots satisfy the extraction gates. '
              f'The geometric-mean cost-estimation ratio is {stats["estimate_geomean"]:.2f}x; whole-extractor time improves '
              f'by {stats["extraction_geomean"]:.3f}x geometrically and {stats["extraction_aggregate"]:.3f}x in aggregate. ')
    abstract+=(f'The measured pipeline ratio is {stats["pipeline_geomean"]:.3f}x geometrically, '
               +('so this experiment does not demonstrate a pipeline-level speedup.\n' if stats['pipeline_geomean']<=1 else 'with the single-pair limitations discussed below.\n'))
    (out/'abstract-results.tex').write_text(abstract.replace('x;',r'$\times$;').replace('x ',r'$\times$ ').replace('x geometrically',r'$\times$ geometrically'))
    successful_pairs=sum(all(c['pipeline'][m]['returncode']==0 for m in ['native','bag']) for c in cases.values())
    lines=[r'\section{Results}',r'\subsection{Correctness and coverage}',
           f'{successful_pairs}/{packet["inventory"]} inventory programs completed both pipelines. {stats["validated_graphs"]}/{len(graphs)} native pass graphs passed complete output equality across the pristine, bag, phase-reference, and oracle lanes. The oracle compared {stats["cost_pick_entries"]:,} class cost/pick entries, counting separate passes. {len(pipeline)} pipeline pairs also had byte-identical intermediate graphs and final outputs.',
           f'The native arithmetic test exercised 4,000 randomized bag operations and 6,400 candidate-cost cases, including repeated keys and unsigned overflow. The benchmark integration tests passed. Failed or timed-out programs, if any, remain in the denominator.']
    if 'additional_repeat_equal_programs' in stats:
        lines.append(f'An additional correctness-only comparison against the preliminary run reproduced both pipeline outputs and their graph snapshots for {stats["additional_repeat_equal_programs"]} programs. Its invalidated external times are not included here.')
    lines += [r'\subsection{Time and memory}',r'\begin{table}[ht]\centering\small',r'\begin{tabular}{lrrrrrr}',r'\toprule',
              r'Suite & $n$ & \shortstack{Estimate\\GM} & \shortstack{Tiger\\GM} & \shortstack{Tiger\\aggregate} & \shortstack{Pipeline\\GM} & \shortstack{RSS\\GM}\\',r'\midrule']
    for g in groups:lines.append(f'{labels[g["suite"]]} & {g["n"]} & {fmt(g["phase"])} & {fmt(g["extractor"])} & {fmt(g["aggregate"])} & {fmt(g["pipeline"])} & {fmt(g["rss"])} \\\\')
    lines += [r'\midrule',f'All & {len(valid)} & {fmt(stats["estimate_geomean"])} & {fmt(stats["extraction_geomean"])} & {fmt(stats["extraction_aggregate"])} & {fmt(stats["pipeline_geomean"])} & {fmt(stats["rss_geomean"])} \\\\',r'\bottomrule\end{tabular}',
              r'\caption{Ratios are original/shared-bag; larger than one means less time or peak RSS for the candidate. GM is a geometric mean over programs. Aggregate Tiger ratio divides sums of three-pass extraction times. RSS uses each program\textquotesingle s maximum standalone peak over its three snapshots.}\end{table}',
              f'Across all programs, frozen-graph extraction totals {native_sum:.3f} s for the original and {bag_sum:.3f} s for shared bags: {native_sum-bag_sum:.3f} s saved ({100*(1-bag_sum/native_sum):.2f} percent). The separately timed target phase totals {old_phase:.3f} s versus {new_phase:.3f} s, an aggregate ratio of {old_phase/new_phase:.2f}.',
              f'The estimator is faster in {stats["phase_wins"]}/{len(valid)} program totals. Whole Tiger has {stats["extraction_wins"]} faster observations and {stats["extraction_slower"]} slower observations. All appear in the appendix. A changed allocation pattern can affect later work, and one pair cannot distinguish small changes from timing variability.',
              r'\begin{figure}[ht]\centering\includegraphics[width=\linewidth]{figures/speedup-distribution.pdf}\caption{The three timing levels must not be conflated. This is a distribution over workloads, not a confidence interval from repeated measurements.}\end{figure}',
              r'\begin{figure}[ht]\centering\includegraphics[width=\linewidth]{figures/paired-extraction.pdf}\caption{Each point is one program\textquotesingle s three-pass extraction sum. Points below the diagonal favor shared bags. Both axes use the same scale.}\end{figure}',
              r'\subsection{Why the pipeline result is smaller}',
              f'The measured pipeline totals are {native_pipeline:.3f} s and {bag_pipeline:.3f} s, an aggregate ratio of {native_pipeline/bag_pipeline:.3f}. Extraction intervals reported by the production driver sum to {inside["native"]:.3f} s and {inside["bag"]:.3f} s. The remaining observed time is {native_pipeline-inside["native"]:.3f} s and {bag_pipeline-inside["bag"]:.3f} s; it includes inference, graph preparation/capture, reconstruction, and other unchanged work.',
              'The targeted cost-estimation phase is only part of extraction, and extraction is only part of compilation. The measured change in the remaining interval is not assigned to cost-bag sharing. '+('The present single-pair experiment therefore supports a targeted cost-estimation improvement and modest extractor-level benefit, but not an end-to-end compiler speedup.' if stats['pipeline_geomean']<=1 else 'The pipeline ratio must be interpreted with this attribution boundary and the single-pair protocol.'),
              f'The maximum arity in serialized inputs is {stats["maximum_arity"]}. Transitive sharing can make bags much larger than this number, so it does not establish a general merge bound. Peak RSS includes unchanged parsing and graph storage, so its measured ratio is reported independently of bag-entry counts.',
              r'The production extraction interval also has a different envelope from standalone replay: the upstream driver writes a temporary input file and captures output through reader threads. Standalone replay reads a prepared file and writes output directly. We measure both envelopes rather than subtract an assumed fixed overhead. Their difference is not assigned quantitatively to a particular cause.']
    ray=next(r for r in valid if r['suite']=='raytrace')
    lines += [r'\subsection{Raytrace in context}',
              f'On pristine-driven raytrace, the three captured passes together take {ray["native_extraction_seconds"]:.3f} s versus {ray["bag_extraction_seconds"]:.3f} s, a ratio of {ray["extraction_speedup"]:.3f}. Its cost-estimation ratio is {ray["estimate_speedup"]:.2f}. This is a new CPU/pristine-input experiment; earlier timings on a saved GPU-produced Pass-3 graph are not pooled with it.',
              f'Its measured full-pipeline times are {ray["native_pipeline_seconds"]:.3f} s versus {ray["bag_pipeline_seconds"]:.3f} s (ratio {ray["pipeline_speedup"]:.3f}). This observed regression remains in the totals despite the faster standalone extractor. We do not claim an integrated raytrace speedup from this run.']
    (out/'results.tex').write_text('\n\n'.join(lines)+'\n')
    appendix=[r'\section{Every workload}',r'All times below sum the three independent, fixed-graph Tiger runs. Pipeline and RSS columns are ratios under the gates in Section 4. LA abbreviates linear-algebra; the CSV and JSON retain full paths, per-pass observations and hashes.',
              r'\begingroup\small\setlength{\tabcolsep}{3pt}',r'\begin{longtable}{p{52mm}rrrrrr}',
              r'\toprule Program & \shortstack{Native\\ms} & \shortstack{Bag\\ms} & \shortstack{Tiger\\ratio} & \shortstack{Estimate\\ratio} & \shortstack{Pipeline\\ratio} & \shortstack{RSS\\ratio}\\\midrule\endfirsthead',
              r'\toprule Program & Native ms & Bag ms & Tiger & Estimate & Pipeline & RSS\\\midrule\endhead',
              r'\bottomrule\endfoot']
    for suite in labels:
        appendix.append(r'\multicolumn{7}{l}{\textbf{'+labels[suite]+r'}}\\*')
        for r in rows:
            if r['suite']!=suite:continue
            name=r['source'].split('/',1)[1].replace('linear-algebra/','LA/')
            values=[r['native_extraction_seconds']*1000 if r['native_extraction_seconds'] is not None else None,
                    r['bag_extraction_seconds']*1000 if r['bag_extraction_seconds'] is not None else None]
            appendix.append(r'\nolinkurl{'+name+'} & '+ ' & '.join([fmt(v,1) for v in values]+[fmt(r[k]) for k in ['extraction_speedup','estimate_speedup','pipeline_speedup','rss_ratio']])+r' \\')
    appendix += [r'\end{longtable}\endgroup']
    (out/'appendix.tex').write_text('\n'.join(appendix)+'\n')
    print(json.dumps(stats,indent=2))


if __name__=='__main__':main()

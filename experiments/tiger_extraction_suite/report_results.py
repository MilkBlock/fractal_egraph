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


def core_report(path, out, original_root):
    p=json.loads((path/'summary.json').read_text())
    assert p['schema']=='tiger-extract-all-fun-roots/v1'
    assert p['passed']==p['attempted']==p['inventory']==96 and p['passed_graphs']==288
    assert p['manifest']['prior_manifest_sha256']==hashlib.sha256((original_root/'manifest.json').read_bytes()).hexdigest()
    rows=p['rows'];t=p['totals'];groups=[]
    labels={'bril':'Bril','polybench':'PolyBench','raytrace':'Raytrace','fenwick':'Fenwick'}
    for key,label in labels.items():
        rs=[r for r in rows if r['suite']==key]
        a=sum(r['native_algorithm_seconds'] for r in rs);b=sum(r['bag_algorithm_seconds'] for r in rs)
        groups.append(dict(suite=label,n=len(rs),native=a,bag=b,gm=gm(r['algorithm_speedup'] for r in rs),aggregate=a/b))
    a=t['native_algorithm_seconds'];b=t['bag_algorithm_seconds']
    result=dict(geomean=p['algorithm_geomean'],aggregate=p['algorithm_aggregate'],native_seconds=a,bag_seconds=b,
                saved_percent=100*(1-b/a),wins=p['wins'],regressions=p['regressions'],groups=groups,
                summary_sha256=hashlib.sha256((path/'summary.json').read_bytes()).hexdigest())
    lines=[r'\section{Complete extraction without communication or parsing}',
           'A follow-up adds identical internal clock boundaries to both extractors and reuses all 288 previously validated graph snapshots. It does not rerun inference. The source, machine, affinity, and input hashes are checked. Every newly selected output again equals the saved pristine output; the historical cost/pick oracle is reused only after verifying that the underlying candidate overlay binary is unchanged.',
           r'\subsection{Exact boundary}',
           r'The interval begins immediately before \tool{extract\_all\_fun\_roots\_tiger(g, roots)} and ends immediately after its return. It includes region discovery and construction, global and candidate cost calculations, Statewalk, final DAG extraction, allocation, merging and reference management. Input reading/JSON parsing precedes the interval. Result serialization, stdout flushing, process startup and teardown are excluded. Candidate diagnostic phase writes inside the interval are removed; the timing record is emitted after output.',
           'This is a direct measurement of the complete algorithm, not process wall time minus guessed communication overhead. Temporary internal graph construction is part of extraction and stays charged. The selected output remains live at the endpoint; its final destruction belongs to process teardown in both lanes.',
           r'\subsection{Results on the same 96 programs}',
           f'All 96 programs and 288 graph pairs pass the new output checks. Complete in-memory extraction totals {a:.3f} s versus {b:.3f} s, a {p["algorithm_aggregate"]:.3f}x aggregate ratio and {100*(1-b/a):.2f} percent less time. The geometric mean over programs is {p["algorithm_geomean"]:.3f}x. {p["wins"]} observations are faster and {len(p["regressions"])} slower. These are single paired observations, with order alternating by program/pass index.',
           r'\begin{table}[ht]\centering\small\begin{tabular}{lrrrrr}\toprule',
           r'Suite & $n$ & Original (s) & Shared bag (s) & GM ratio & Aggregate\\\midrule']
    for g in groups:lines.append(f'{g["suite"]} & {g["n"]} & {g["native"]:.3f} & {g["bag"]:.3f} & {g["gm"]:.3f} & {g["aggregate"]:.3f} \\\\')
    lines += [r'\midrule',f'All & 96 & {a:.3f} & {b:.3f} & {p["algorithm_geomean"]:.3f} & {p["algorithm_aggregate"]:.3f} \\\\',
              r'\bottomrule\end{tabular}\caption{Complete in-memory extraction, excluding read/parse and serialize/write. Ratios are original/shared-bag.}\end{table}']
    ray=next(r for r in rows if r['suite']=='raytrace')
    lines.append(f'For raytrace, the three-pass algorithm total falls from {ray["native_algorithm_seconds"]:.3f} s to {ray["bag_algorithm_seconds"]:.3f} s ({ray["algorithm_speedup"]:.3f}x). Its read/parse totals are {ray["native_read_parse_seconds"]:.3f} s and {ray["bag_read_parse_seconds"]:.3f} s. The previously reported integrated raytrace regression is retained; this new isolated timing does not overturn it.')
    # A single new pair supplies all components in this plot; no cross-run subtraction.
    components=[('read_parse_seconds','Read + parse','#9AA7B2'),('algorithm_seconds','Complete algorithm','#205D85'),
                ('serialize_write_seconds','Serialize + write','#D3963C')]
    fig,ax=plt.subplots(figsize=(6.5,2.6));left=[0.,0.]
    for key,label,color in components:
        values=[t[f'{m}_{key}'] for m in ['native','bag']]
        ax.barh(['Original','Shared bag'],values,left=left,label=label,color=color)
        left=[l+v for l,v in zip(left,values)]
    residual=[t[f'{m}_wall_seconds']-left[i] for i,m in enumerate(['native','bag'])]
    assert min(residual)>=0
    ax.barh(['Original','Shared bag'],residual,left=left,label='Other process overhead',color='#DFE3E6')
    ax.invert_yaxis();ax.set_xlabel('Sum over all 288 snapshots (seconds)');ax.legend(frameon=False,ncol=2,fontsize=8,loc='lower center',bbox_to_anchor=(.5,1.02));ax.grid(axis='x',alpha=.15)
    fig.tight_layout();fig.savefig(out/'figures/core-components.pdf');fig.savefig(out/'figures/core-components.png',dpi=180);plt.close(fig)
    lines += [r'\begin{figure}[ht]\centering\includegraphics[width=\linewidth]{figures/core-components.pdf}\caption{Components from the same follow-up runs. Other process overhead includes startup, teardown, scheduling and wrapper overhead; it is not labeled entirely as communication.}\end{figure}',
              f'Across the suite, read/parse alone totals {t["native_read_parse_seconds"]:.3f} s and {t["bag_read_parse_seconds"]:.3f} s. Serialize/write totals {t["native_serialize_write_seconds"]:.3f} s and {t["bag_serialize_write_seconds"]:.3f} s. Process elapsed totals in this follow-up are {t["native_wall_seconds"]:.3f} s and {t["bag_wall_seconds"]:.3f} s ({t["native_wall_seconds"]/t["bag_wall_seconds"]:.3f}x). Parsing therefore dilutes the observed process-level gain, while the large remaining parsing cost is unchanged by cost-bag sharing.',
              'The original process/pipeline table and this follow-up are separate measurement sessions. Their times are not spliced or subtracted to infer missing phases. Geometric and aggregate ratios answer different questions; no GPU transfer or online compiler speedup is claimed.']
    (out/'core-results.tex').write_text('\n\n'.join(lines)+'\n')
    app=[r'\section{Every workload: in-memory extraction}',
         r'Times in milliseconds, summed over the three pass snapshots in the follow-up. Algorithm includes the full Tiger call. Read/parse and serialize/write are measured separately. LA abbreviates linear-algebra. N is original, B is shared bag. All outputs match the pristine reference.',
         r'\begingroup\small\setlength{\tabcolsep}{3pt}',r'\begin{longtable}{p{48mm}rrrrrrr}',
         r'\toprule Program & \shortstack{N\\algorithm} & \shortstack{B\\algorithm} & Ratio & \shortstack{N\\parse} & \shortstack{B\\parse} & \shortstack{N\\write} & \shortstack{B\\write}\\\midrule\endfirsthead',
         r'\toprule Program & N algo & B algo & Ratio & N parse & B parse & N write & B write\\\midrule\endhead',r'\bottomrule\endfoot']
    for key,label in labels.items():
        app.append(r'\multicolumn{8}{l}{\textbf{'+label+r'}}\\*')
        for r in rows:
            if r['suite']!=key:continue
            name=r['source'].split('/',1)[1].replace('linear-algebra/','LA/')
            vals=[fmt(r[m+'_algorithm_seconds']*1000,2) for m in ['native','bag']]+[fmt(r['algorithm_speedup'])]
            vals += [fmt(r[m+'_'+k]*1000,2) for k in ['read_parse_seconds','serialize_write_seconds'] for m in ['native','bag']]
            app.append(r'\nolinkurl{'+name+'} & '+' & '.join(vals)+r' \\')
    app.append(r'\end{longtable}\endgroup');(out/'core-appendix.tex').write_text('\n'.join(app)+'\n')
    return result


def main():
    p=argparse.ArgumentParser();p.add_argument('--input',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--preliminary',type=Path);p.add_argument('--core',type=Path)
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
    if args.core:
        stats['complete_algorithm']=core_report(args.core,out,root)
        core=stats['complete_algorithm']
        addition=f'A separate complete in-memory extraction measurement excludes reading, parsing and output: {core["geomean"]:.3f}x geometrically and {core["aggregate"]:.3f}x in aggregate.\n'
        with (out/'abstract-results.tex').open('a') as f:f.write(addition.replace('x ',r'$\times$ '))
    else:
        for name in ['core-results.tex','core-appendix.tex']:(out/name).unlink(missing_ok=True)
    (out/'report-stats.json').write_text(json.dumps(stats,indent=2)+'\n')
    print(json.dumps(stats,indent=2))


if __name__=='__main__':main()

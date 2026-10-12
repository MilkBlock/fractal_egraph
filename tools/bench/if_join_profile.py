#!/usr/bin/env python3
"""Explain the sparse If layout regression on frozen native benchmark inputs.

Reuses native timing/graph verification helpers. Plan/block profiles are
diagnostics, not uninstrumented speed measurements. The two modified libraries
are workload-specific counterfactuals, never installed as compiler optimizations.
"""
import argparse
import copy
import csv
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

from math_virtual import ROOT,measure
from math_graph_equal import verify
from eggcc_if import make_input,check_branches
from spine_ablation import manifests
sys.path.insert(0,str(ROOT))
from tools.cell_spine.markers import render
from tools.cell_spine.terms import walk

RUNNER=ROOT/'target/release/examples/native_math_baseline'
HOT=('impl_6_5_cover_5','impl_7_5_cover_10')
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
seconds=lambda d:d['secs']+d['nanos']/1e9


def blocks(stderr):
    result=[];pending={};ordinals={}
    for line in stderr.splitlines():
        if not line.startswith('EGGLOG_JOIN_WORK\t'):continue
        parts=line.split('\t');name=bytes.fromhex(parts[1]).decode()
        metrics=dict(s.split('=',1) for s in parts[10:]);entries=metrics.pop('stage_entries')
        values=[None if x=='-' else int(x) for x in parts[6:10]]
        row=dict(rule=name,kind=parts[2],block=int(parts[3]),stages=int(parts[4]),seconds=int(parts[5])/1e9,
            **dict(zip(('materialized_keys','materialized_rows','message_columns','value_columns'),values)),
            counters={k:int(v) for k,v in metrics.items()},stage_entries=[int(x) for x in entries.split(',') if x])
        p=row['counters']
        if p['candidate_rows']!=p['  candidate_rows enumerated (for_each)']+p['  candidate_rows from lookups']:
            raise ValueError('probe accounting identity failed')
        if row['kind']!='plan' and len(row['stage_entries'])!=row['stages']:
            raise ValueError('missing actual stage counts')
        row['query']=ordinals.get(name,0);result.append(row)
        if row['kind']=='plan':
            children=pending.pop(name,[])
            for key in p:
                if sum(r['counters'][key] for r in children)>p[key]:
                    raise ValueError('child block work exceeds plan work')
            ordinals[name]=row['query']+1
        else:pending.setdefault(name,[]).append(row)
    if pending:raise ValueError('incomplete plan profile')
    return result


def counterfactuals(base,folder):
    lib=(base/'joint/library.egg').read_text()
    rules=json.loads((base/'joint/marker-runtime.json').read_text())['rules']
    result={name:lib for name in ('drop_hot','canonical_hot')};seen=set()
    for rule in rules:
        if rule['name'] not in HOT:continue
        old=render(rule)
        if lib.count(old)!=1:raise ValueError('frozen rule text changed')
        edited=copy.deepcopy(rule);n=0
        for fact in edited['body']:
            for root in fact['args'] if fact['kind']=='eq' else [fact['expr']]:
                for e in walk(root):
                    if e['kind']=='call' and e['head']=='Spine-Single':
                        e['head']='Spine-Cano-Single';n+=1
        if n!=1:raise ValueError('expected exactly one fallback Single read')
        result['canonical_hot']=result['canonical_hot'].replace(old,render(edited))
        result['drop_hot']=result['drop_hot'].replace(old,'; diagnostic omission: '+rule['name'])
        seen.add(rule['name'])
    if seen!=set(HOT):raise ValueError('hot rules are absent')
    for name,lib in result.items():(folder/f'{name}.egg').write_text('; DIAGNOSTIC ONLY: not a generally equivalent compiled library.\n'+lib)


def summarize(result,block_rows):
    v=result['verification'];report=v['run_report']
    all_tables=v['native_tables_before_export']
    tables={k:n for k,n in all_tables.items() if k.startswith('Spine-') or k in v['restored']['tables']}
    rules=[dict(rule=n,matches=report['num_matches_per_rule'].get(n,0),search_apply_ms=seconds(t)*1000)
           for n,t in report['search_and_apply_time_per_rule'].items()]
    rules.sort(key=lambda r:-r['search_apply_ms'])
    return dict(run_ms=result['run_seconds']*1000,matches=result['matches'],rules=rules,
        phases_ms={k:sum(seconds(t)*1000 for t in report[k].values()) for k in
            ('search_and_apply_time_per_ruleset','merge_time_per_ruleset','rebuild_time_per_ruleset')},
        probes=v['run_probe_counters'],blocks=block_rows,shape=v['restored'],
        tables_before_export=tables,summary_tables=v['summary_tables'],
        omitted_tables=dict(count=len(all_tables)-len(tables),rows=sum(all_tables.values())-sum(tables.values())),
        total_table_rows_before_export=v['native_total_rows_before_export'])


def write_reports(report,out):
    with (out/'rules.csv').open('w') as f:
        w=csv.DictWriter(f,fieldnames=['variant','rule','matches','search_apply_ms'],lineterminator='\n');w.writeheader()
        for mode,m in report['variants'].items():
            for r in m['rules']:w.writerow(dict(variant=mode,**r))
    with (out/'blocks.csv').open('w') as f:
        keys=['variant','rule','query','kind','block','stages','milliseconds','materialized_keys','materialized_rows','message_columns','value_columns','probes','candidate_rows','stage_entries']
        w=csv.DictWriter(f,fieldnames=keys,lineterminator='\n');w.writeheader()
        for mode,m in report['variants'].items():
            for r in m['blocks']:
                w.writerow(dict(variant=mode,**{k:r[k] for k in keys if k in r and k!='stage_entries'},milliseconds=r['seconds']*1000,
                    probes=r['counters']['probes'],candidate_rows=r['counters']['candidate_rows'],stage_entries=json.dumps(r['stage_entries'])))
    lines=['# Sparse If: native Join diagnosis','',
        'Diagnostic runs with query plans and opt-in block counters. Frozen libraries and inputs; no Bake or Gurobi in these runs.','',
        '| Variant | Run ms | Search/apply ms | Merge ms | Rebuild ms | Matches | Candidate rows |','|---|---:|---:|---:|---:|---:|---:|']
    for mode,m in report['variants'].items():
        p=m['phases_ms'];lines.append(f'| {mode} | {m["run_ms"]:.3f} | {p["search_and_apply_time_per_ruleset"]:.3f} | {p["merge_time_per_ruleset"]:.3f} | {p["rebuild_time_per_ruleset"]:.3f} | {m["matches"]} | {m["probes"]["candidate_rows"]} |')
    lines+=['','The full rule and block tables are in rules.csv / blocks.csv. Block stage-entry counts use actual runtime order choices; static plans are saved in plans.json.',
        'Block timings are inclusive of their stage execution. Plan rows include child blocks; do not sum plan + block rows. Materialized rows count retained partial bindings, not all attempted combinations.',
        'No per-row clocks are added. Selected rules get counters per block and stage-entry counts. This instrumented run is for attribution; use the earlier non-profile ablation for timing comparisons.',
        'drop_hot removes two zero-match rules only as a counterfactual for this input. It is not valid global pruning. canonical_hot changes two lookup table names only to test that hypothesis; it is not a compiler fix.']
    (out/'README.md').write_text('\n'.join(lines)+'\n')
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--ablation-root',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--skip-build',action='store_true')
    p.add_argument('--ifs',type=int,default=1000,help='1000 reuses the original input; other sizes use the same sparse generator')
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    if a.ifs<1:p.error('--ifs must be positive')
    if not a.skip_build:
        subprocess.run(['cargo','build','--offline','--release','--features','native-join-work-profile','--example','native_math_baseline'],cwd=ROOT,check=True)
    os.environ['RAYON_NUM_THREADS']='1';os.environ['EGGLOG_JOIN_WORK_RULES']=','.join(HOT)
    base=a.ablation_root.resolve()/'eggcc_if';source=base/'fresh-1000'
    ablation=json.loads((a.ablation_root/'report.json').read_text())
    training=ablation['training']['eggcc_if']['variants']
    for name in ('old','joint'):
        if sha(base/name/'library.egg')!=training[name]['library_sha256']:raise ValueError('frozen library changed')
    counterfactuals(base,out)
    expected_kinds=None
    if a.ifs!=1000:
        data=json.loads((base/'prepare/input.json').read_text())
        initial,expected_kinds=make_input(data,a.ifs,10)
        source=out/'workload'
        manifests(data,initial,{'old':base/'old/library.egg','joint':base/'joint/library.egg'},source)
    report=dict(schema='if-join-diagnosis/v1',threads=1,diagnostic_only=True,
        commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),
        binary_sha256=sha(RUNNER),ablation_report_sha256=sha(a.ablation_root/'report.json'),
        profile_rules=list(HOT),ifs=a.ifs,variants={},measurements={},counterfactuals_are_not_production_fixes=True)
    plans={}
    for mode in ('native','old','joint','canonical_hot','drop_hot'):
        template='native' if mode=='native' else mode+'_export' if mode in ('old','joint') else 'joint_export'
        manifest=json.loads((source/f'{template}.json').read_text())
        if mode in ('canonical_hot','drop_hot'):manifest['environment']=str(out/f'{mode}.egg')
        manifest_path=out/f'{mode}.json';manifest_path.write_text(json.dumps(manifest,indent=2)+'\n')
        graph=out/f'{mode}-graph.json'
        m=measure([RUNNER,'bench-plans',manifest_path,graph],out/f'{mode}-profile')
        profiles=blocks((out/f'{mode}-profile.stderr').read_text())
        if mode in ('old','joint','canonical_hot') and {r['rule'] for r in profiles}!=set(HOT):
            raise ValueError('block profiles missing; rebuild with native-join-work-profile')
        eq_sorts=list(m['result']['verification']['restored']['sorts'])
        reference=(source/'native-graph.json') if a.ifs==1000 else out/'native-graph.json'
        equal=verify(reference,graph,eq_sorts)
        if expected_kinds is not None and check_branches(graph,a.ifs,10)!=expected_kinds:
            raise ValueError('branch semantics changed')
        if mode!='native' and m['result']['matches']!=2*a.ifs+2*((a.ifs+9)//10):raise ValueError('logical matches changed')
        summary=summarize(m['result'],profiles);summary['equivalence']=equal
        report['variants'][mode]=summary
        report['measurements'][mode]={k:v for k,v in m.items() if k!='result'}
        # Keep complete compiled plans separate from the concise diagnosis.
        plans[mode]=m['result']['verification']['run_report']
        write_reports(report,out)
        print(mode,round(summary['run_ms'],3),summary['probes']['candidate_rows'],flush=True)
    (out/'plans.json').write_text(json.dumps(plans,indent=2)+'\n')
    print(out/'README.md')


if __name__=='__main__':main()

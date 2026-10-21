#!/usr/bin/env python3
"""Reusable entry point to existing native/GPU experiments; standard library only."""
from __future__ import annotations
import argparse
import csv
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import sys

REPO = Path(__file__).resolve().parents[3]
SHARED = Path('/Users/mineralsteins/Repos/egg_related')
SNAPSHOT = SHARED / 'egg_layout/out/raytrace-interactive-20261010/pass-3-flat.egg'
TIGER = SHARED / 'check_statewalk/target/release/tiger'
WORKER = Path('out/cell-locality-worker/matrix')
SAVED = Path('experiments/raytrace_pass3_gpu/results/stable-choice')

def history_candidates(repo):
    """Only inspect workflow packets; never walk/export the large saved graphs."""
    return sorted((repo / 'out').glob('*/workflow.json'),
                  key=lambda p: p.stat().st_mtime, reverse=True)

def matching_history(repo, snapshot, policy, joint, all_rules=False):
    """Historical controls are evidence, not a claim about the current binary."""
    source_hash = sha(snapshot)
    for path in history_candidates(repo):
        try:
            w = load(path)
            if (w.get('mode') != 'run' or w.get('returncode') != 0
                    or w.get('snapshot_sha256') != source_hash or w.get('policy') != policy):
                continue
            r = load(path.parent / 'report.json')
            if bool(r.get('gpu_without_joint')) != joint or bool(r.get('gpu',{}).get('extended_queries')) != all_rules or not metrics(r)['timing_eligible']:
                continue
            yield path.parent, w, r
        except (OSError, ValueError, KeyError, TypeError):
            continue  # Incomplete/old diagnostic packets are not timing controls.

def inference_identity(a, repo, worker, env):
    binary = repo / 'target/analyzer/release/examples/raytrace_stable_bench'
    resolve = lambda p: p if p.is_absolute() else repo / p
    if not binary.is_file() or not worker.is_file():
        return None
    controls = {}
    if getattr(a, 'reuse_controls', None):
        folder = resolve(a.reuse_controls)
        for name in ['report.json', 'workflow.json', 'graphs/native.control.json', 'graphs/cpu.control.json'] + (['graphs/gpu-off.control.json'] if a.joint_sat else []):
            file = folder/name
            if not file.is_file(): return None
            controls[name] = sha(file)
    configuration = {k: v for k, v in sorted(env.items())
                     if k.startswith(('RAYON_', 'EGGLOG_', 'CELL_', 'MTL_', 'DYLD_', 'OMP_'))}
    return dict(version=1, snapshot=sha(resolve(a.snapshot)), binary=sha(binary),
                worker=sha(worker), policy=a.policy, joint=a.joint_sat, all_rules=a.all_rules, controls=controls,
                machine=[platform.node(), platform.system(), platform.release(), platform.machine()],
                environment_sha256=hashlib.sha256(json.dumps(configuration, sort_keys=True).encode()).hexdigest())

def extraction_identity(a, repo):
    resolve = lambda p: p if p.is_absolute() else repo / p
    paths = [resolve(a.tiger), repo / 'target/analyzer/release/examples/tiger_output_check',
             repo / 'experiments/raytrace_pass3_gpu/extract.py']
    if a.reference_output:
        paths.append(resolve(a.reference_output))
    if not all(p.is_file() for p in paths):
        return None
    return [sha(p) for p in paths]

def reusable_result(a, repo, worker, env):
    identity = inference_identity(a, repo, worker, env)
    if identity is None:
        return None
    extraction = None if a.no_extract else extraction_identity(a, repo)
    for directory, workflow, _ in matching_history(
            repo, a.snapshot if a.snapshot.is_absolute() else repo / a.snapshot, a.policy, a.joint_sat, a.all_rules):
        if workflow.get('inference_identity') != identity:
            continue
        if not a.no_extract:
            try:
                timing = load(directory / 'extraction/timing.json')
                provenance = directory / 'workflow-extract.json'
                ew = load(provenance) if provenance.is_file() else workflow
                if (extraction is None or ew.get('extraction_identity') != extraction
                        or not load(directory / 'extraction/quality.json').get('passed')
                        or len(timing.get('runs', [])) != 3):
                    continue
            except (OSError, ValueError, KeyError, TypeError):
                continue
        return directory
    return None

def sha(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()

def load(path):
    return json.loads(Path(path).read_text())

def exact(value):
    return isinstance(value, dict) and value.get('exact_rows_values_equivalence_visibility') is True

def metrics(r, quality=None, extraction=None):
    """Never infer equivalence from costs, counts, or presence of a JSON object."""
    ok = exact(r.get('same_commit_verification'))
    audit = r.get('gpu', {}).get('audit')
    cpu = r['sorted_commit_cpu']['runtime']['run_seconds']
    gpu = r['gpu']['runtime']['run_seconds']
    control=r.get('gpu_without_joint')
    joint_ok=not control or exact(r.get('joint_verification'))
    eligible = ok and joint_ok and r.get('benchmark_finished') is True and audit is False and not r.get('gpu',{}).get('query_timing_profile',False) and not r.get('gpu',{}).get('cells',{}).get('profile',False) and cpu > 0 and gpu > 0
    result = dict(exact_cpu_gpu=ok, exact_fused_native_gpu=exact(r.get('verification')),
                  timing_eligible=eligible, inference_speedup=cpu / gpu if eligible else None,
                  extraction_passed=bool(quality and quality.get('passed') is True),
                  inference_plus_tiger_speedup=None, joint_speedup=None)
    if eligible and control and control.get('audit') is False:
        result['joint_speedup']=control['runtime']['run_seconds']/gpu
    if eligible and result['extraction_passed'] and extraction:
        times = {v['lane']: v['seconds'] for v in extraction['runs'] if v['returncode'] == 0}
        if 'cpu' in times and 'gpu' in times:
            result['inference_plus_tiger_speedup'] = (cpu + times['cpu']) / (gpu + times['gpu'])
    return result

def report(path, fresh=False):
    path = Path(path)
    print('本次执行结果。' if fresh else '读取保存结果，未重新测量。')
    if path.is_dir():
        choices = ['report.json', 'stable-time.json', 'results.json', 'summary.json', 'time.json', 'check.json', 'audit.json', 'diff.json', 'cpu-pair.json']
        path = next((path / n for n in choices if (path / n).is_file()), path / 'report.json')
    r = load(path)
    if 'sorted_commit_cpu' in r:
        committed = path.name == 'stable-time.json'
        qpath = path.parent / ('stable-quality.json' if committed else 'extraction/quality.json')
        tpath = path.parent / ('stable-extraction.json' if committed else 'extraction/timing.json')
        m = metrics(r, load(qpath) if qpath.is_file() else None, load(tpath) if tpath.is_file() else None)
        print('\n| 路径 | 主推理 s |\n|---|---:|')
        for name, sec in [('原生融合', r['native']['run_seconds']),
                          ('CPU 同提交协议', r['sorted_commit_cpu']['runtime']['run_seconds']),
                          ('GPU', r['gpu']['runtime']['run_seconds'])]:
            print(f'| {name} | {sec:.6f} |')
        if r.get('gpu_without_joint'):
            print(f"| GPU 关闭 joint sat-skip | {r['gpu_without_joint']['runtime']['run_seconds']:.6f} |")
        print('\nCPU/GPU 整图：', m['exact_cpu_gpu'], '；融合原生/GPU 整图：', m['exact_fused_native_gpu'])
        pair = path.parent / 'extraction/cpu-gpu-quality.json'
        if pair.is_file(): print('Tiger same-commit CPU/GPU:', load(pair).get('passed'))
        print('实际 Tiger 选定 DAG 检查：', m['extraction_passed'] if qpath.is_file() else '尚未运行')
        print('有效推理加速比：', f"{m['inference_speedup']:.4f}x" if m['timing_eligible'] else '不报告（检查未通过，或这是 audit/阶段计时）')
        if m['inference_plus_tiger_speedup'] is not None:
            print(f"推理＋Tiger：{m['inference_plus_tiger_speedup']:.4f}x")
        if r.get('reused_controls'): print('复用历史对照：', r['reused_controls']['directory'], '；本次仅运行 GPU 候选并重新核对完整图。')
        print('以上不含序列化、诊断、共享输入准备；worker setup 见原始报告。当前 fork，不是 pristine。')
        print('由 Cell handler 完成的饱和边界（含显式 dataflow 推进）：', r['gpu']['runtime'].get('skipped_saturations', 0))
        print('Cell supplied queries/bindings:', r['gpu']['runtime'].get('cell_query_rules', 0), r['gpu']['runtime'].get('cell_query_bindings', 0))
        if r.get('gpu_without_joint'):
            print('joint 单独收益（GPU 关闭÷开启）：',f"{m['joint_speedup']:.4f}x" if m['joint_speedup'] is not None else '当前检查或计时模式不报告')
            print('joint 实际命中/回退：',r['gpu'].get('saturation_diagnostics',{}).get('joint',{}).get('stats'))
            comparison=((r['gpu'].get('saturation_diagnostics') or {}).get('joint') or {}).get('comparison_closure')
            if comparison:
                print('Smaller 内部饱和闭包（union_proposals 是等价请求，不是新增事实）：',comparison)
        if not m['exact_cpu_gpu']:
            print('失败原因：', r.get('same_commit_verification_error'))
    elif 'transfer_bench_verified' in r:
        print('多候选 Cell 文件加载性能，CPU 小基准；每条路径单次运行，不是 Pass 3。')
        print('| 路径 | setup ms | input ms | run ms | 合计 ms |\n|---|---:|---:|---:|---:|')
        for lane in r['lanes']:
            print('| '+lane['lane']+' | '+' | '.join(f"{lane[k]*1000:.3f}" for k in ['setup_seconds','input_seconds','run_seconds','total_seconds'])+' |')
        print('逐阶段完整图门槛：', all(exact(v.get(k)) for v in r['verification'] for k in ['native','loaded','joint']))
    elif 'transfer_run_complete' in r or ('library' in r and 'loaded_programs' in r.get('runtime', {}).get('stats', {})):
        print('加载 Bake 文件的运行结果；输入断言与镜像核对：', r.get('passed') is True)
        print('阶段数：', len(r['boundaries']), '；加载程序数：', r['runtime']['stats']['loaded_programs'],
              '；重新搜索接线计划：', r['runtime']['stats']['program_compilations'])
        print('这是执行报告，不是独立原生整图对照或计时加速比；整图回归见 transfer_artifact。')
    elif 'port_transition_verified' in r:
        print('已有 Cell 的端口接续，实际原生提交；比较索引读取次数，不报告时间加速比。')
        for p in r['phases']:
            print(p['label'], '无复用/有复用/直接复用：',
                  p['index_reads_without_reuse'], p['index_reads_with_reuse'], p['reused_reads'],
                  '整图：', exact(p['same_submit']) and exact(p['reuse_off_on']))
    elif 'joint_sat_verified' in r:
        print('联合 terms 饱和证明；实际 CPU/GPU 调度和原生提交，未报告加速比。')
        print(r['scope'])
        for lane in r['lanes']:
            checked=all(exact(p.get('verification')) for p in lane['phases'])
            last=lane['phases'][-1]['runtime']['saturation_diagnostics']['joint']
            print(lane['lane'], '整图检查：', checked, '；联合证明：', last['stats'])
    elif 'lanes' in r:
        print(f"Math sat-skip：{r['instances']} 实例；模式 {r['mode']}。不是 raytrace。")
        print('| 路径 | 边界总秒数 | smooth 逻辑匹配 | 全边界验证 |\n|---|---:|---:|---|')
        for lane in r['lanes']:
            phases = lane['phases']
            checked = bool(phases) and all(exact(p.get('verification')) for p in phases)
            print(f"| {lane['lane']} | {sum(p['seconds'] for p in phases):.6f} | {lane['smooth_matches']} | {checked if lane['lane'] != 'native' else 'reference'} |")
    elif 'boundaries' in r:
        failed = [x for x in r['boundaries'] if x.get('equal') is False or x.get('cpu_updated') != x.get('gpu_updated')]
        print('差异定位（不是计时）：', json.dumps(failed[0] if failed else r['boundaries'][-1] if r['boundaries'] else '尚无完成的边界', ensure_ascii=False))
    elif 'exact_graph' in r:
        print('CPU 自对照（不是计时平均）：', exact(r['exact_graph']), r.get('graph_error'))
    elif 'cell_spine_templates' in r:
        print('参数化 terms 更新程序：CPU 正确性原型；不报告加速比或整 Cell 饱和跳过。')
        print('逐阶段核对通过：', parameterized_gate(r), '；阶段数：', len(r['phases']))
        print('Bop spine 模板：', r['cell_spine_templates'], '；规则程序数：', r['rule_kernels'])
        print('具体闭包目录条目：', r['runtime']['concrete_closure_catalog_entries'])
        print('运行统计：', json.dumps(r['runtime']['stats'], ensure_ascii=False))
    elif 'runtime' in r:
        print('Native-frontier audit；不是独立最终图认证，也不是计时：', r.get('complete'))
        print('Query audit scope:', r['runtime'].get('audit_scope', 'all queries'))
        print('核对查询次数：', sum(x['stats'].get('audit_queries', 0) for x in r['runtime'].get('rules', [])))
    else:
        raise ValueError('unknown report schema: ' + str(path))
    print('报告：', path)

def interaction_report(paths):
    """Counted factorial experiment; never publish its instrumented seconds."""
    expected = {'s0r0': (False, False), 's0r1': (False, True),
                's1r0': (True, False), 's1r1': (True, True), 'sat-only': (True, True)}
    if not {'s0r0','s0r1','s1r0','s1r1'} <= paths.keys():
        raise ValueError('four factorial corners are required')
    records, rule_counts, identity = {}, {}, None
    fields = ['eligible_seeds', 'refuted_seeds', 'productive_seeds']
    for name, path in paths.items():
        r, w = load(path/'report.json'), load(path/'workflow.json')
        g = r['gpu']; work = g['cell_work']; sat, refute = expected[name]
        if not exact(r.get('same_commit_verification')) or r.get('benchmark_finished') is not True:
            raise ValueError('unverified graph: '+name)
        if g.get('cells',{}).get('profile') is not True:
            raise ValueError('seed counters were not enabled: '+name)
        if bool(g.get('saturation_diagnostics')) != sat or g['cells']['refutation'] != refute:
            raise ValueError('incorrect experimental switches: '+name)
        if g.get('cell_query_reuse') != (name != 'sat-only'):
            raise ValueError('incorrect Cell frontier switch: '+name)
        if refute and (not g['cells']['refutation_signatures'] or g['cells']['refutation_domains']):
            raise ValueError('refutation configuration differs: '+name)
        key = [w['snapshot_sha256'],w['binary_sha256'],w['worker_sha256'],
               w['policy'],r['schedule'],r['assisted_rulesets'],r['same_commit_verification']]
        if identity is None: identity=key
        elif key != identity: raise ValueError('input, binary, worker, schedule or graph differs: '+name)
        if not all(f in work for f in fields): raise ValueError('missing seed counters; use layout 13+: '+name)
        rules={(x['ruleset'],x['rule']):x['stats'] for x in g['rules']}
        if len(rules)!=len(g['rules']): raise ValueError('duplicate rule identity')
        for f in fields:
            if sum(x[f] for x in rules.values()) != work[f]:
                raise ValueError('per-rule seed accounting differs: '+name)
        if any(type(work[f]) is not int or work[f]<0 for f in fields) or work['refuted_seeds']+work['productive_seeds']>work['eligible_seeds']:
            raise ValueError('invalid seed outcomes: '+name)
        rule_counts[name]=rules
        records[name]=dict(directory=str(path),provenance=w,work=work,
                          rejection_rate=work['refuted_seeds']/work['eligible_seeds'] if work['eligible_seeds'] else None,
                          productive_rate=work['productive_seeds']/work['eligible_seeds'] if work['eligible_seeds'] else None,
                          cell_query_bindings=g['runtime']['cell_query_bindings'],
                          cell_query_rules=g['runtime']['cell_query_rules'],
                          exact_same_commit=r['same_commit_verification'],
                          exact_fused_native=exact(r.get('verification')))
    for a,b in [('s0r0','s0r1'),('s1r0','s1r1')]:
        for f in ['eligible_seeds','productive_seeds']:
            if records[a]['work'][f]!=records[b]['work'][f]:
                raise ValueError('refutation changed seed population counts: '+a+'/'+b)
    def mix(a,b):
        left,right=rule_counts[a],rule_counts[b]
        if left.keys()!=right.keys(): raise ValueError('rule universe changed')
        common=[k for k in left if left[k]['eligible_seeds'] and right[k]['eligible_seeds']]
        removed=[k for k in left if left[k]['eligible_seeds'] and not right[k]['eligible_seeds']]
        identical=[k for k in left if all(left[k][f]==right[k][f] for f in fields)]
        total=sum(left[k]['eligible_seeds'] for k in common)
        return dict(reference=a,candidate=b,common_rules=len(common),identical_rules=len(identical),
                    identical_nonempty_rules=sum(left[k]['eligible_seeds']>0 for k in identical),
                    common_reference_rate=sum(left[k]['refuted_seeds'] for k in common)/total if total else None,
                    common_candidate_fixed_weight_rate=sum(left[k]['eligible_seeds']*right[k]['refuted_seeds']/right[k]['eligible_seeds'] for k in common)/total if total else None,
                    removed_rules=len(removed),removed_totals={f:sum(left[k][f] for k in removed) for f in fields},
                    removed=[dict(ruleset=k[0],rule=k[1],stats=left[k]) for k in removed],
                    changed_common=[dict(ruleset=k[0],rule=k[1],before={f:left[k][f] for f in fields},after={f:right[k][f] for f in fields}) for k in common if any(left[k][f]!=right[k][f] for f in fields)])
    contrasts={}
    for metric in ['join_live_row_visits','join_probes']:
        v={k:r['work'][metric] for k,r in records.items()}
        contrasts[metric]=dict(cell_saving_without_refutation=v['s0r0']-v['s1r0'],
             cell_saving_with_refutation=v['s0r1']-v['s1r1'],
             refutation_saving_without_cell=v['s0r0']-v['s0r1'],
             refutation_saving_with_cell=v['s1r0']-v['s1r1'],
             combined_fraction_saved=1-v['s1r1']/v['s0r0'] if v['s0r0'] else None,
             superadditive_saving=v['s1r0']+v['s0r1']-v['s0r0']-v['s1r1'])
    mixtures=[mix('s0r1','s1r1')]
    if 'sat-only' in paths: mixtures += [mix('s0r1','sat-only'),mix('sat-only','s1r1')]
    return dict(scope='Actual full Pass-3 work-count experiment. Profiled seconds are not speedup evidence. This ablates certified services, not training data or Bake quality.',
                records=records,contrasts=contrasts,query_mix=mixtures)

def join_depth_report(paths):
    """Depth is matched non-anchor atoms; loops/branches are not probabilities."""
    result=interaction_report(paths)
    required={'entries','link_visits','live_rows','prefixes','key_checks','refutations',
              'matched_leaves','domain_checks','signature_refutations','deepest_seeds'}
    parsed={}; depths={}; fields=None
    for name,path in paths.items():
        g=load(path/'report.json')['gpu']; names=g.get('join_depth_fields',[])
        if set(names)!=required or len(names)!=len(required):
            raise ValueError('missing full Join depth schema; collect layout 14 profile: '+name)
        if fields is None: fields=names
        elif fields!=names: raise ValueError('depth schema/order differs')
        rules={}; totals=[]
        def empty(): return {f:0 for f in fields}
        for q in g['rules']:
            stats=q['stats']; n=q['atoms']; raw=stats.get('depth_work',[])
            if raw and len(raw)!=n: raise ValueError('depth vector differs from rule arity')
            rows=[dict(zip(fields,v)) for v in raw] if raw else [empty() for _ in range(n)]
            if any(len(v)!=len(fields) for v in raw): raise ValueError('depth field width differs')
            if any(type(v) is not int or v<0 for row in rows for v in row.values()): raise ValueError('invalid depth count')
            if sum(row['deepest_seeds'] for row in rows)!=stats['eligible_seeds']:
                raise ValueError('deepest-depth histogram differs from seed count')
            if rows:
                for f,seed_field in [('entries','eligible_seeds'),('prefixes','eligible_seeds'),
                                     ('refutations','refuted_seeds'),('deepest_seeds','refuted_seeds')]:
                    if rows[0][f]!=stats[seed_field]: raise ValueError('anchor-depth accounting differs')
            for d,row in enumerate(rows):
                if row['refutations']>row['prefixes']: raise ValueError('too many refutations')
                if d and not row['prefixes']<=row['live_rows']<=row['link_visits']:
                    raise ValueError('prefix/row/link accounting differs')
                if d+1<n and rows[d+1]['entries']!=row['prefixes']-row['refutations']:
                    raise ValueError('next loop does not conserve surviving prefixes')
                if row['matched_leaves']>row['prefixes']-row['refutations'] or (d+1<n and row['matched_leaves']):
                    raise ValueError('invalid matched leaves')
                while len(totals)<=d: totals.append(empty())
                for f,v in row.items(): totals[d][f]+=v
            rules[(q['ruleset'],q['rule'])]=dict(seeds=stats['eligible_seeds'],rows=rows)
        work=g['cell_work']
        for field,global_field in [('link_visits','join_link_visits'),('live_rows','join_live_row_visits'),
                                  ('key_checks','refutation_checks'),('domain_checks','domain_checks'),
                                  ('signature_refutations','signature_refuted_branches')]:
            if sum(r[field] for r in totals)!=work[global_field]: raise ValueError('depth totals differ: '+field)
        # Legacy join_probes is cleared on output retry; per-depth counters
        # retain all attempts. The compact-path counters are authoritative here.
        result['records'][name]['compact_probe_attempts']=sum(r['entries'] for r in totals[1:])
        result['records'][name]['legacy_final_attempt_probes']=work['join_probes']
        if sum(r['refutations'] for r in totals)!=work['refuted_branches']+work['domain_refuted_branches']:
            raise ValueError('depth refutation totals differ')
        seeds=work['eligible_seeds']; visits=work['join_live_row_visits']; rejects=sum(r['refutations'] for r in totals)
        bins=[]
        for d,row in enumerate(totals):
            bins.append(dict(depth=d,**row,
                rejection_rate_given_prefix=row['refutations']/row['prefixes'] if row['prefixes'] else None,
                row_visit_share=row['live_rows']/visits if visits else None,
                refutation_event_share=row['refutations']/rejects if rejects else None,
                entries_per_seed=row['entries']/seeds if seeds else None,
                deepest_seed_probability=row['deepest_seeds']/seeds if seeds else None,
                reach_probability=sum(x['deepest_seeds'] for x in totals[d:])/seeds if seeds else None))
        parsed[name]=rules;depths[name]=bins
    def contrast(a,b):
        left,right=parsed[a],parsed[b]
        common=[k for k in left if left[k]['seeds'] and right[k]['seeds']]
        weight=sum(left[k]['seeds'] for k in common); size=max(len(depths[a]),len(depths[b]))
        def weighted(source,d,field,tail=False):
            if not weight:return None
            def val(k):
                rows=source[k]['rows']
                return sum(r[field] for r in rows[d:]) if tail else (rows[d][field] if d<len(rows) else 0)
            return sum(left[k]['seeds']*val(k)/source[k]['seeds'] for k in common)/weight
        same_seeds=all(left[k]['seeds']==right[k]['seeds'] for k in left)
        fixed=[];raw=[]
        for d in range(size):
            fixed.append(dict(depth=d,
                before_rows_per_seed=weighted(left,d,'live_rows'),after_rows_per_seed=weighted(right,d,'live_rows'),
                before_entries_per_seed=weighted(left,d,'entries'),after_entries_per_seed=weighted(right,d,'entries'),
                before_reach_probability=weighted(left,d,'deepest_seeds',True),after_reach_probability=weighted(right,d,'deepest_seeds',True)))
            va=depths[a][d]['live_rows'] if d<len(depths[a]) else 0
            vb=depths[b][d]['live_rows'] if d<len(depths[b]) else 0
            raw.append(dict(depth=d,before=va,after=vb,saved=va-vb,reduction=(1-vb/va) if va else None))
        return dict(before=a,after=b,common_rules=len(common),same_per_rule_seed_counts=same_seeds,
                    fixed_rule_mix=fixed,raw_row_savings=raw)
    result.update(depth_fields=fields,depths=depths,depth_contrasts={
        'cell_effect_with_refutation':contrast('s0r1','s1r1'),
        'refutation_effect_with_cell':contrast('s1r0','s1r1'),
        'refutation_effect_without_cell':contrast('s0r0','s0r1')})
    result['depth_scope']='Actual compact GPU Join. Depth 0 is a valid multi-atom anchor; depth d attempts the d-th non-anchor atom. Deepest-seed bins count each seed evaluation once, including retries. Entry counts can exceed seed counts due to branching; they are not reach probabilities.'
    return result

def parser():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--repo', type=Path, default=REPO)
    p.add_argument('--dry-run', action='store_true')
    sub = p.add_subparsers(dest='mode', required=True)
    q = sub.add_parser('report'); q.add_argument('input', type=Path, nargs='?', default=SAVED)
    for mode in ['interaction','depth']:
        q = sub.add_parser(mode, help='read saved factorial/depth counters; no runs')
        for name in ['s0r0','s0r1','s1r0','s1r1']: q.add_argument('--'+name,type=Path,required=True)
        q.add_argument('--sat-only',type=Path);q.add_argument('--output',type=Path)
        if mode=='depth':q.add_argument('--csv',type=Path)
    q = sub.add_parser('history', help='reuse historical evidence; no build or execution')
    q.add_argument('--snapshot', type=Path, default=SNAPSHOT)
    q.add_argument('--policy', choices=['stable', 'original'], default='stable')
    q.add_argument('--joint-sat', action='store_true')
    q.add_argument('--all-rules', action='store_true')
    q.add_argument('--limit', type=int, default=3)
    for mode in ['check', 'build', 'run', 'extract', 'audit', 'cpu-pair', 'diff', 'sat-skip', 'parameterized', 'transition', 'transfer-bench', 'joint-sat', 'test']:
        q = sub.add_parser(mode)
        q.add_argument('--worker', type=Path, default=WORKER)
        q.add_argument('--no-build', action='store_true')
        if mode in ['check', 'run', 'audit', 'cpu-pair', 'diff']:
            q.add_argument('--snapshot', type=Path, default=SNAPSHOT)
        if mode == 'joint-sat': q.add_argument('--snapshot', type=Path)
        if mode in ['check', 'run', 'extract']:
            q.add_argument('--tiger', type=Path, default=TIGER)
        if mode in ['run', 'audit', 'cpu-pair', 'diff', 'sat-skip', 'parameterized', 'transition', 'transfer-bench', 'joint-sat']:
            q.add_argument('--output', type=Path)
        if mode in ['run', 'extract']:
            q.add_argument('--reference-output', type=Path)
        if mode == 'extract': q.add_argument('input', type=Path)
        if mode == 'run':
            q.add_argument('--policy', choices=['stable', 'original'], default='stable')
            q.add_argument('--no-extract', action='store_true')
            q.add_argument('--joint-sat', action='store_true')
            q.add_argument('--all-rules', action='store_true')
            q.add_argument('--reuse-controls', type=Path, help='reuse saved exact native/CPU/GPU-off controls; all-rules only')
            q.add_argument('--refresh', action='store_true', help='remeasure even if an identical verified result exists')
        if mode == 'audit':
            q.add_argument('--cell-only', action='store_true')
            q.add_argument('--three-lane-check', action='store_true')
            q.add_argument('--all-rules', action='store_true')
            q.add_argument('--joint-sat', action='store_true')
        if mode == 'diff':
            q.add_argument('--inspect'); q.add_argument('--open')
            g = q.add_mutually_exclusive_group(); g.add_argument('--sequential', action='store_true'); g.add_argument('--sort-hoist', action='store_true')
        if mode == 'sat-skip':
            q.add_argument('--checked', action='store_true'); q.add_argument('--instances', type=int)
        if mode == 'transfer-bench':
            q.add_argument('--instances', type=int, default=512)
            q.add_argument('--library', type=Path, default=Path('experiments/parameterized_cell/results/transfers.egg'))
    return p

def commands(a, repo, worker, out):
    """Build an inspectable plan without executing or creating anything."""
    resolve = lambda p: p if p.is_absolute() else repo / p
    binary = lambda n: str(repo / 'target/analyzer/release/examples' / n)
    source = str(resolve(a.snapshot)) if getattr(a, 'snapshot', None) else None
    if a.mode == 'run':
        c = [[binary('raytrace_stable_bench'), source, str(worker), str(out)] + (['--original'] if a.policy == 'original' else [])]
        if a.joint_sat: c[0].append('--joint-sat')
        if a.all_rules: c[0].append('--all-rules')
        if not a.no_extract:
            c += [[sys.executable, str(repo / 'experiments/raytrace_pass3_gpu/extract.py'), str(out), '--tiger', str(resolve(a.tiger))]]
    elif a.mode == 'extract':
        c = [[sys.executable, str(repo / 'experiments/raytrace_pass3_gpu/extract.py'), str(out), '--tiger', str(resolve(a.tiger))]]
    elif a.mode == 'audit' and a.all_rules:
        if a.three_lane_check: raise ValueError('use run --all-rules for independent graph checks')
        c = [[binary('raytrace_stable_bench'), source, str(worker), str(out), '--all-rules', '--cell-audit-only' if a.cell_only else '--audit-only'] + (['--joint-sat'] if a.joint_sat else [])]
    elif a.mode == 'audit':
        if a.joint_sat or a.cell_only: raise ValueError('--joint-sat/--cell-only audit requires --all-rules')
        c = [[binary('raytrace_pass3_bench'), source, str(worker), str(out), 'check' if a.three_lane_check else 'audit']]
    elif a.mode == 'cpu-pair':
        c = [[binary('raytrace_pass3_bench'), source, str(worker), str(out), 'cpu-pair']]
    elif a.mode == 'diff':
        c = [[binary('raytrace_pass3_diff'), source, str(worker), str(out)] + (['--sequential'] if a.sequential else ['--sort-hoist'] if a.sort_hoist else [])]
    elif a.mode == 'sat-skip':
        n = a.instances if a.instances is not None else (32 if a.checked else 512)
        if n < 1: raise ValueError('instances must be positive')
        c = [[binary('sat_skip_bench'), str(worker), str(out), str(n), 'check' if a.checked else 'time']]
    elif a.mode == 'parameterized':
        c = [[binary('parameterized_cell'), str(out)]]
    elif a.mode == 'transition':
        c = [[binary('cell_transition'), str(out)]]
    elif a.mode == 'transfer-bench':
        if a.instances < 1: raise ValueError('instances must be positive')
        c = [[binary('transfer_bench'), str(resolve(a.library)), str(out), str(a.instances)]]
    elif a.mode == 'joint-sat':
        c = [[binary('joint_sat_bench'), str(worker), str(out)] + ([source] if source else [])]
    elif a.mode == 'test':
        c = [['cargo', 'test', '--offline', '--manifest-path', 'egglog/egglog-bridge/Cargo.toml', '--target-dir', 'target/analyzer', '--lib', 'cell_worklist_tests', '--', '--test-threads=1'],
             ['cargo', 'test', '--offline', '--manifest-path', 'cell-assisted-egglog/Cargo.toml', '--test', 'stable_choice', '--test', 'hybrid', '--test', 'table_live', '--test', 'full_gpu', '--target-dir', 'target/analyzer', '--', '--test-threads=1'],
             ['cargo', 'test', '--offline', '--manifest-path', 'cell-assisted-egglog/Cargo.toml', '--lib', 'stable_choice::tests', '--target-dir', 'target/analyzer'],
             ['cargo', 'test', '--offline', '--test', 'whole_cell', '--test', 'whole_cell_automatic', '--test', 'sat_skip', '--test', 'parameterized_cell', '--test', 'joint_sat', '--test', 'scalar_cells', '--test', 'scalar_closure', '--test', 'transfer_artifact', '--example', 'raytrace_stable_bench', '--target-dir', 'target/analyzer', '--', '--test-threads=1']]
    else: c = []
    if getattr(a, 'reference_output', None): c[-1] += ['--reference-output', str(resolve(a.reference_output))]
    return c

def parameterized_gate(r):
    phases = r.get('phases', [])
    return (r.get('passed') is True and bool(phases)
            and all(exact(p.get('same_submit')) and exact(p.get('fused_native'))
                    and p.get('same_per_rule_binding_counts') is True
                    and p.get('mirror_rows_checked', 0) > 0 for p in phases))

def saved_gate(mode, out, three_lane=False):
    """Some old diagnostic binaries save a failed check but still return zero."""
    if mode == 'run' or (mode == 'audit' and three_lane):
        r=load(out/'report.json')
        return exact(r.get('same_commit_verification')) and (not r.get('gpu_without_joint') or exact(r.get('joint_verification')))
    if mode == 'cpu-pair': return exact(load(out/'cpu-pair.json').get('exact_graph'))
    if mode == 'audit': return load(out/'audit.json').get('complete') is True
    if mode == 'extract': return load(out/'extraction/quality.json').get('passed') is True
    if mode == 'parameterized': return parameterized_gate(load(out/'results.json'))
    if mode == 'transition':
        r = load(out/'results.json')
        return (r.get('port_transition_verified') is True and r.get('passed') is True
                and bool(r.get('phases')) and all(exact(p.get('same_submit')) and exact(p.get('reuse_off_on')) for p in r['phases']))
    if mode == 'transfer-bench':
        r=load(out/'results.json')
        return r.get('transfer_bench_verified') is True and bool(r.get('verification')) and all(
            exact(v.get(k)) for v in r['verification'] for k in ['native','loaded','joint'])
    if mode == 'joint-sat':
        r=load(out/'results.json')
        return r.get('joint_sat_verified') is True and len(r['lanes'])==2 and all(l['phases'] and all(exact(p.get('verification')) for p in l['phases']) for l in r['lanes'])
    if mode == 'diff':
        rows = load(out/'diff.json')['boundaries']
        return bool(rows) and rows[-1].get('equal') is True
    if mode == 'sat-skip':
        lanes = load(out/'results.json')['lanes']
        checked = [p for lane in lanes if lane['lane'] != 'native' for p in lane['phases']]
        return bool(checked) and all(exact(p.get('verification')) for p in checked)
    return True

def main(argv=None):
    a = parser().parse_args(argv); repo = a.repo.resolve()
    resolve = lambda p: p if p.is_absolute() else repo / p
    print('执行 checkout：', repo)
    if a.mode == 'report': report(resolve(a.input)); return 0
    if a.mode in ['interaction','depth']:
        paths={name:resolve(getattr(a,name)) for name in ['s0r0','s0r1','s1r0','s1r1']}
        if a.sat_only: paths['sat-only']=resolve(a.sat_only)
        result=join_depth_report(paths) if a.mode=='depth' else interaction_report(paths)
        print('读取已有计数，不重新执行；不使用带计数器的时间证明加速。')
        for name,r in result['records'].items():
            print(name, 'candidate row visits:',r['work']['join_live_row_visits'], 'seed rejection:',f"{100*r['rejection_rate']:.3f}%" if r['rejection_rate'] is not None else 'N/A')
        print('固定规则权重的比较：',json.dumps([{k:v for k,v in m.items() if k not in ['removed','changed_common']} for m in result['query_mix']],ensure_ascii=False))
        if a.mode=='depth':
            for name,rows in result['depths'].items():
                print(name,'depth / loop entries / row visits / refutations / reach probability')
                for row in rows:
                    if row['entries'] or row['refutations']:
                        print(row['depth'],row['entries'],row['live_rows'],row['refutations'],row['reach_probability'])
        if a.mode=='depth' and a.csv and not a.dry_run:
            dest=resolve(a.csv);dest.parent.mkdir(parents=True,exist_ok=True)
            names=['configuration','depth']+result['depth_fields']+['reach_probability','deepest_seed_probability','refutation_event_share','row_visit_share','rejection_rate_given_prefix','entries_per_seed']
            with dest.open('x',newline='') as f:
                writer=csv.DictWriter(f,fieldnames=names,lineterminator="\n");writer.writeheader()
                for name,rows in result['depths'].items():
                    for row in rows:writer.writerow(dict(configuration=name,**row))
        if a.output and not a.dry_run:
            dest=resolve(a.output);dest.parent.mkdir(parents=True,exist_ok=True)
            with dest.open('x') as f:json.dump(result,f,indent=2);f.write('\n')
        return 0
    if a.mode == 'history':
        if a.limit < 1:
            raise ValueError('--limit must be positive')
        print('假设机器稳定；复用历史计时，没有重新测量。旧版本不代表当前代码。')
        found = 0
        for directory, w, r in matching_history(repo, resolve(a.snapshot), a.policy, a.joint_sat, a.all_rules):
            m = metrics(r)
            print(json.dumps(dict(directory=str(directory), head=w.get('source_head'),
                                  dirty=bool(w.get('dirty_files')),
                                  cpu_seconds=r['sorted_commit_cpu']['runtime']['run_seconds'],
                                  gpu_seconds=r['gpu']['runtime']['run_seconds'],
                                  gpu_off_seconds=(r.get('gpu_without_joint') or {}).get('runtime', {}).get('run_seconds'),
                                  joint_speedup=m['joint_speedup']), ensure_ascii=False))
            found += 1
            if found >= a.limit: break
        print(f'{found} 份可用记录。用 report 目录读取完整验证；不按最快的一次挑基准。')
        return 0
    worker = resolve(a.worker)
    if a.mode == 'check':
        files = [repo / 'examples/raytrace_stable_bench.rs', repo / 'experiments/raytrace_pass3_gpu/extract.py', resolve(a.snapshot), resolve(a.tiger)]
        missing = [str(p) for p in files if not p.is_file()]
        missing += [tool for tool in ['cargo', 'xcrun'] if not shutil.which(tool)]
        print('缺失：', missing or '无')
        print('worker 缓存：', worker, worker.is_file(), '；存在不代表与源码一致。')
        return int(bool(missing))
    out = resolve(a.input) if a.mode == 'extract' else resolve(getattr(a, 'output', None) or Path('out') / ('cell-bench-' + a.mode + '-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')))
    if a.mode == 'run' and a.no_extract and a.reference_output:
        raise ValueError('--reference-output requires extraction')
    if not a.dry_run:
        if getattr(a, 'snapshot', None) and not resolve(a.snapshot).is_file(): raise FileNotFoundError(resolve(a.snapshot))
        if a.mode in ['run','audit','cpu-pair','diff','sat-skip','parameterized','transition','transfer-bench','joint-sat'] and out.exists(): raise ValueError('使用新的 output 目录：' + str(out))
        if a.mode == 'extract':
            for lane in ['native','cpu','gpu']:
                if not (out/'graphs'/f'{lane}.json').is_file(): raise FileNotFoundError(out/'graphs'/f'{lane}.json')
            if (out/'extraction').exists(): raise ValueError('提取目录已存在；使用 report 查看，不能覆盖')
    plan = commands(a, repo, worker, out)
    target = {'build': 'raytrace_stable_bench', 'run': 'raytrace_stable_bench', 'audit': 'raytrace_pass3_bench', 'cpu-pair': 'raytrace_pass3_bench', 'diff': 'raytrace_pass3_diff', 'sat-skip': 'sat_skip_bench', 'parameterized': 'parameterized_cell', 'transition': 'cell_transition', 'transfer-bench': 'transfer_bench', 'joint-sat': 'joint_sat_bench'}.get(a.mode)
    if a.mode == 'audit' and a.all_rules: target = 'raytrace_stable_bench'
    targets = [target] if target else []
    if a.mode in ['build', 'extract'] or (a.mode == 'run' and not a.no_extract): targets += ['tiger_output_check']
    build = ['cargo', 'build', '--offline', '--release', '--target-dir', 'target/analyzer'] + [x for t in targets for x in ['--example', t]]
    env = dict(os.environ, RAYON_NUM_THREADS='1', EGGLOG_PROBE_COUNTS_OFF='1', CELL_MATRIX_WORKER=str(worker))
    if getattr(a, 'reuse_controls', None):
        if not a.all_rules: raise ValueError('--reuse-controls requires --all-rules')
        folder=resolve(a.reuse_controls)
        if not a.dry_run:
            previous=load(folder/'workflow.json')
            if previous.get('snapshot_sha256')!=sha(resolve(a.snapshot)) or previous.get('policy')!=a.policy: raise ValueError('control input/choice policy differs')
        env['CELL_BENCH_REUSE_CONTROLS']=str(folder)
    if a.mode == 'test': env['RAYON_NUM_THREADS'] = '4'
    for name in ['inspect', 'open']:
        if getattr(a, name, None): env['PASS3_DIFF_' + name.upper()] = getattr(a, name)
        else: env.pop('PASS3_DIFF_' + name.upper(), None)
    history = []
    def execute(cmd, log=None):
        cmd = list(map(str, cmd)); history.append(cmd)
        print(shlex.join(cmd), flush=True)
        if a.dry_run: return 0
        if log:
            with log.open('a') as f: return subprocess.run(cmd, cwd=repo, env=env, stdout=f, stderr=subprocess.STDOUT).returncode
        return subprocess.run(cmd, cwd=repo, env=env).returncode
    if not a.no_build:
        if targets and execute(build): return 1
        if a.mode not in ['extract', 'cpu-pair', 'parameterized', 'transition', 'transfer-bench']:
            fingerprint = worker.with_name(worker.name + '.skill-build.json')
            source = repo / 'cell-assisted-egglog/matrix.swift'
            cached = load(fingerprint) if fingerprint.is_file() else {}
            valid = worker.is_file() and cached.get('source_sha256') == sha(source) and cached.get('binary_sha256') == sha(worker)
            if not valid:
                if not a.dry_run: worker.parent.mkdir(parents=True, exist_ok=True)
                if execute(['xcrun', 'swiftc', '-O', source, '-o', worker]): return 1
                if not a.dry_run: fingerprint.write_text(json.dumps(dict(source_sha256=sha(source), binary_sha256=sha(worker))) + '\n')
    if a.mode == 'build': return 0
    if a.dry_run:
        for c in plan: execute(c)
        if a.mode == 'diff': print('定位环境：', {k:v for k,v in env.items() if k.startswith('PASS3_DIFF_')})
        return 0
    if a.mode == 'run' and not a.refresh:
        cached = reusable_result(a, repo, worker, env)
        if cached is not None:
            print('输入、二进制、worker、策略及配置一致，复用已验证记录：', cached)
            print('没有重新执行推理或创建目标目录；强制重新测量用 --refresh。')
            report(cached)
            return 0
    if a.mode not in ['test','extract']:
        out.parent.mkdir(parents=True, exist_ok=True)
    log = None if a.mode == 'test' else out.with_name(out.name + '.log')
    status = 0
    for i, c in enumerate(plan):
        rc = execute(c, log); status = status or rc
        if rc:
            # A saved graph failure is still useful input for extraction quality.
            if not (a.mode == 'run' and i == 0 and (out / 'report.json').is_file() and load(out / 'report.json').get('benchmark_finished') is True): break
    if a.mode != 'test' and out.is_dir():
        try:
            if not saved_gate(a.mode, out, getattr(a,'three_lane_check',False)): status = status or 1
        except (FileNotFoundError, KeyError): status = status or 1
        provenance = dict(mode=a.mode, policy=getattr(a, 'policy', None), commands=history, returncode=status,
                          repo=str(repo), source_head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(),
                          dirty_files=subprocess.check_output(['git','status','--short'],cwd=repo,text=True),
                          no_build=a.no_build, worker_sha256=sha(worker) if worker.is_file() else None,
                          snapshot_sha256=sha(resolve(a.snapshot)) if getattr(a,'snapshot',None) else None,
                          binary_sha256={str(p):sha(p) for t in targets if (p:=repo/'target/analyzer/release/examples'/t).is_file()})
        if a.mode == 'run':
            provenance['inference_identity'] = inference_identity(a, repo, worker, env)
        if a.mode == 'transfer-bench':
            provenance['library_sha256'] = sha(resolve(a.library))
            provenance['instances'] = a.instances
            provenance['fixture_sha256'] = {name:sha(repo/'experiments/parameterized_cell'/name) for name in ['terms.egg','competing_updates.egg']}
        if a.mode == 'extract' or (a.mode == 'run' and not a.no_extract):
            provenance['extraction_identity'] = extraction_identity(a, repo)
        (out / ('workflow-extract.json' if a.mode == 'extract' else 'workflow.json')).write_text(json.dumps(provenance, indent=2) + '\n')
        try: report(out, fresh=True)
        except FileNotFoundError: print('未生成完整报告，日志：', log)
    return status

if __name__ == '__main__':
    try: sys.exit(main())
    except (OSError, ValueError, KeyError) as e:
        print(str(e), file=sys.stderr); sys.exit(1)

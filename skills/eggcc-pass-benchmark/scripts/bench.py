#!/usr/bin/env python3
"""Fast entry point to existing eggcc profiling; standard library only."""
import argparse
import datetime
import json
import pathlib
import hashlib
import shlex
import subprocess
import sys
from zoneinfo import ZoneInfo

DEFAULT = pathlib.Path('/Users/mineralsteins/Repos/egg_related/egg_layout')
VERSIONED = pathlib.Path(__file__).resolve().parents[3]
PRODUCTION = DEFAULT.parent/'check_statewalk'
p = argparse.ArgumentParser()
p.add_argument('--repo', type=pathlib.Path)
p.add_argument('--dry-run', action='store_true')
s = p.add_subparsers(dest='mode', required=True)
r = s.add_parser('report'); r.add_argument('--input', type=pathlib.Path)
for name in ['check', 'run']:
    q = s.add_parser(name)
    q.add_argument('--library', type=pathlib.Path, default=pathlib.Path('out/ripsat-coverage-remeasure-merged/train/library.json'))
    q.add_argument('--profile-binary', type=pathlib.Path, default=pathlib.Path('out/eggcc-pass-audit/eggcc-profile'))
    if name == 'run':
        q.add_argument('--output', type=pathlib.Path)
        q.add_argument('--source', type=pathlib.Path, action='append')
for name in ['saturate-check','saturate-build','saturate-run','saturate-report']:
    q=s.add_parser(name)
    q.add_argument('--production-repo',type=pathlib.Path,default=PRODUCTION)
    q.add_argument('--profile-binary',type=pathlib.Path)
    q.add_argument('--output',type=pathlib.Path)
    q.add_argument('--source',type=pathlib.Path,action='append')
    q.add_argument('--input',type=pathlib.Path)
a = p.parse_args()
# Preserve the old coverage/cache defaults. New schedule tools live alongside
# this versioned skill; main checkout may not contain them yet.
repo = (a.repo or (VERSIONED if a.mode.startswith('saturate-') else DEFAULT)).resolve()
print('执行 checkout：',repo)
def resolve(path): return path if path.is_absolute() else repo / path

def call(cmd):
    print(shlex.join(map(str,cmd)),flush=True)
    if not a.dry_run:subprocess.run(list(map(str,cmd)),cwd=repo,check=True)

def saturation():
    production=resolve(a.production_repo)
    if a.mode=='saturate-report':
        folder=resolve(a.input or pathlib.Path('experiments/eggcc_passes/raytrace_saturation/results'))
        print('读取保存结果，未重新测量；只读汇总，不覆盖原报告。')
        call([sys.executable,repo/'experiments/eggcc_passes/saturation_report.py',folder,'--no-write'])
        return
    if a.mode=='saturate-build':
        dest=resolve(a.output or pathlib.Path('out')/('eggcc-saturate-profiler-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')))
        call([sys.executable,repo/'experiments/eggcc_passes/build_profile.py','--schedule-only','--production-repo',production,'--output',dest])
        return
    candidates=[repo/'out/eggcc-saturate-profiler-v2/eggcc-schedule-profile',repo/'out/eggcc-schedule-profiler/eggcc-schedule-profile',repo/'out/eggcc-saturate-profiler/eggcc-schedule-profile']
    binary=resolve(a.profile_binary) if a.profile_binary else next((p for p in candidates if p.is_file()),candidates[1])
    print('schedule profiler：',binary)
    sources=[resolve(x) for x in a.source] if a.source else [production/'benchmarks/passing/raytrace/raytrace.rs']
    if not a.dry_run:
        required=[binary,repo/'experiments/eggcc_passes/run.py',*sources]
        missing=[str(p) for p in required if not p.is_file()]
        if missing:raise SystemExit('缺少：\n'+'\n'.join(missing)+'\n先用 saturate-build；无需 coverage library、flatten 或 ripsat_coverage。')
        digest=hashlib.sha256(binary.read_bytes()).hexdigest()
        provenance=binary.parent/'provenance.json'
        if provenance.is_file():
            expected=json.loads(provenance.read_text()).get('profile_binary_sha256')
            if expected and expected!=digest:raise SystemExit('profiler 二进制与其 provenance 指纹不符')
        print('profiler sha256：',digest,'；存在/指纹匹配不代表与当前源码重新构建一致。')
    if a.mode=='saturate-check':return
    dest=resolve(a.output or pathlib.Path('out')/('eggcc-saturate-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')))
    call([sys.executable,repo/'experiments/eggcc_passes/run.py','--timing-only','--production-repo',production,'--profile-binary',binary,'--output',dest]+[v for source in sources for v in ['--source',source]])
    if not a.dry_run:
        summary=json.loads((dest/'saturation-summary.json').read_text())
        if not summary['workloads']:raise SystemExit('没有有效 schedule_profile；不能报告为 0 秒 Saturate')
        print('本次单次生产三 pass 测量：',dest/'saturation-summary.json')

def report(path, fresh=False):
    d = json.loads(path.read_text())
    print(f"数据日期：{d['date']}。" + ('本次单次测量。' if fresh else '读取保存结果，未重新测量。'))
    print('\n时间：毫秒。执行含准备；search 含匹配管理；insert/merge 合并计时。\n')
    print('| 样例 | Pass | 执行 | Search | RHS | Insert＋merge | Rebuild | Extraction wall |')
    print('|---|---:|---:|---:|---:|---:|---:|---:|')
    keys = ['execution_ms','search_and_binding_ms','rhs_ms','insert_merge_ms','rebuild_ms','extraction_wall_ms']
    for row in d['rows']:
        assert row['coverage_native_control_equal'], 'coverage control failed'
        print('| '+row['workload']+' | '+str(row['pass'])+' | '+' | '.join(f"{row['timing'][k]:.2f}" for k in keys)+' |')
    print('\n覆盖率：'+d['library_scope']+'\n')
    print('| 样例 | Pass | Expr 节点 | Expr e-class | Constructor 行 | 完整 LHS |')
    print('|---|---:|---:|---:|---:|---:|')
    for row in d['rows']:
        values=[row['coverage'][k]['percent'] for k in ['expr_enodes','expr_eclasses','constructor_rows','pre_step_full_lhs']]
        print('| '+row['workload']+' | '+str(row['pass'])+' | '+' | '.join('—' if v is None else f'{v:.2f}%' for v in values)+' |')
    print('\n计时引擎：'+d['timing_engine']+'\n覆盖引擎：'+d['coverage_engine'])
    print('\n数据：'+str(path))

if a.mode.startswith('saturate-'):
    saturation()
elif a.mode == 'report':
    report(resolve(a.input or pathlib.Path('experiments/eggcc_passes/results.json')))
else:
    required = [resolve(a.profile_binary), repo.parent/'check_statewalk/target/release/flatten', repo/'target/release/examples/ripsat_coverage', resolve(a.library)]
    missing=[str(x) for x in required if not x.is_file()]
    if missing:
        print('缺少以下文件；按 references/build.md 只补缺失项：\n'+'\n'.join(missing));sys.exit(1)
    lib=json.loads(resolve(a.library).read_text())
    print(f"工具齐全；冻结库 {len(lib['patterns'])} 个模式条目、{len(lib['signatures'])} 个签名。缓存存在不代表已核验当前源码版本。")
    if a.mode == 'run':
        stamp=datetime.datetime.now(ZoneInfo('Asia/Shanghai')).strftime('%Y%m%d-%H%M%S')
        dest=resolve(a.output or pathlib.Path('out')/('eggcc-passes-'+stamp))
        call([sys.executable,str(repo/'experiments/eggcc_passes/run.py'),'--output',str(dest),'--library',str(resolve(a.library)),'--profile-binary',str(resolve(a.profile_binary))]+[arg for source in (a.source or []) for arg in ['--source',str(resolve(source))]])
        if not a.dry_run:report(dest/'summary.json', fresh=True)

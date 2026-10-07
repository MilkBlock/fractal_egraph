#!/usr/bin/env python3
"""Fast entry point to existing eggcc profiling; standard library only."""
import argparse
import datetime
import json
import pathlib
import subprocess
import sys
from zoneinfo import ZoneInfo

DEFAULT = pathlib.Path('/Users/mineralsteins/Repos/egg_related/egg_layout')
p = argparse.ArgumentParser()
p.add_argument('--repo', type=pathlib.Path, default=DEFAULT)
s = p.add_subparsers(dest='mode', required=True)
r = s.add_parser('report'); r.add_argument('--input', type=pathlib.Path)
for name in ['check', 'run']:
    q = s.add_parser(name)
    q.add_argument('--library', type=pathlib.Path, default=pathlib.Path('out/ripsat-coverage-remeasure-merged/train/library.json'))
    q.add_argument('--profile-binary', type=pathlib.Path, default=pathlib.Path('out/eggcc-pass-audit/eggcc-profile'))
    if name == 'run': q.add_argument('--output', type=pathlib.Path)
a = p.parse_args(); repo = a.repo.resolve()
def resolve(path): return path if path.is_absolute() else repo / path

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

if a.mode == 'report':
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
        subprocess.run([sys.executable,str(repo/'experiments/eggcc_passes/run.py'),'--output',str(dest),'--library',str(resolve(a.library)),'--profile-binary',str(resolve(a.profile_binary))],cwd=repo,check=True)
        report(dest/'summary.json', fresh=True)

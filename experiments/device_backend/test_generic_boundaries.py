"""Capacity, failure, and dynamic-batch boundaries, separate from timing."""
import argparse,copy,json,subprocess
from pathlib import Path
from test_generic_cells import core_for
from run_generic_cells import run_library
from verify_native_source import verify
from eggfuse.compiler.targets.metal.generic_cell import lower

def one(source,build,out,name):
    path=out/(name+'.egg');path.write_text(source)
    core=core_for(path,build);initial=core.pop('initial')
    return path,dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)])

def tests(build,out):
    out.mkdir(parents=True,exist_ok=False);checks={}
    source='''(datatype E (N i64)) (ruleset grow)
(rule ((= x (N n)) (< n 100)) ((N (+ n 1))) :ruleset grow)
(N 0) (run-schedule (repeat 5 (run grow)))'''
    path,lib=one(source,build,out,'growth')
    other=copy.deepcopy(lib['instances'][0]);other['unit']=1;other['initial'][0]['values'][0]['value']=100
    lib['instances'].append(other)
    r=run_library(lib,out/'build',out/'growth',capacity=2,match_capacity=2)
    assert r['complete']
    assert [a['executed_instances'] for a in r['programs'][0]['attempts']]==[2,1,1]
    checks['growth']=verify(path,build,out/'growth/program-0/cell-0',out/'growth-native')
    other_path=out/'no-growth.egg';other_path.write_text(source.replace('(N 0)','(N 100)'))
    checks['unchanged_instance_not_replayed']=verify(other_path,build,out/'growth/program-0/cell-1',out/'no-growth-native')
    no=run_library(lib,out/'build',out/'no-grow',capacity=2,match_capacity=2,grow=False)
    assert not no['complete']
    assert next(s for s in no['programs'][0]['instances'] if s['unit']==0)['meta'][4]==1
    path,lib=one(source.replace('repeat 5','repeat 40'),build,out,'history')
    r=run_library(lib,out/'build',out/'history',capacity=64)
    assert r['complete'] and len(r['programs'][0]['instances'][0]['history'])==40
    checks['history_growth']=verify(path,build,out/'history/program-0/cell-0',out/'history-native')
    # A failed RHS may have earlier private effects, but cannot complete/export
    # a successful endpoint. No overflow is turned into a false numeric guard.
    path,lib=one('''(datatype E (N i64) (Made)) (ruleset bad)
(rule ((= x (N n))) ((Made) (N (+ n 1))) :ruleset bad)
(N 9223372036854775807) (run bad 1)''',build,out,'overflow')
    r=run_library(lib,out/'build',out/'overflow')
    assert not r['complete'] and r['programs'][0]['instances'][0]['meta'][4]==5
    checks['rhs_overflow_rejected']=True
    source='''(datatype E (N i64)) (relation Q (E)) (ruleset idle) (ruleset make)
(rule ((= x (N n))) ((Q x)) :ruleset make)
(N 0) (run idle 1) (check (Q (N 0))) (run make 1)'''
    path,lib=one(source,build,out,'failed-check')
    core=lib['programs'][0]
    for node in core['schedule']:
        if node.pop('assert',False):
            rs=next(s for s in core['rulesets'] if s['name']==node['run'])
            names={a['call']['table'] for r in rs['rules'] for a in r['head'] if 'table' in a.get('call',{})}
            assert len(names)==1;node['assert_table']=names.pop()
    r=run_library(lib,out/'build',out/'failed-check')
    assert not r['complete'] and r['programs'][0]['instances'][0]['meta'][4]==8
    native=subprocess.run([str((build/'frontend').resolve()),str(path.resolve()),'--native-full','--export-to',str((out/'failed-native').resolve())],capture_output=True)
    assert native.returncode!=0 and b'Check' in native.stderr
    checks['failed_intermediate_check_not_rescued']=True
    # Anonymous until conditions are rejected, not silently omitted.
    core=dict(lib['programs'][0],initial=lib['instances'][0]['initial'],schedule=[{'run':'make','until_run':'probe'}])
    try:lower(core)
    except ValueError:checks['until_refused']=True
    else:raise AssertionError('until dropped')
    # Interned strings share one batch dictionary without confusing identity.
    path,lib=one('''(datatype E (N String)) (relation Seen (E)) (ruleset s)
(rule ((= x (N name))) ((Seen x)) :ruleset s)
(N "a") (run s 1)''',build,out,'strings')
    other=copy.deepcopy(lib['instances'][0]);other['unit']=1;other['initial'][0]['values'][0]['value']='b';lib['instances'].append(other)
    r=run_library(lib,out/'build',out/'strings');assert r['complete']
    for i,name in enumerate(['a','b']):
        f=out/f'string-{i}.egg';f.write_text(path.read_text().replace('(N "a")',f'(N "{name}")'))
        checks[f'string-{i}']=verify(f,build,out/'strings/program-0'/f'cell-{i}',out/f'string-native-{i}')
    (out/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();tests(a.build,a.output)

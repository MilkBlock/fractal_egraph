"""Check real Raytrace constructors and is-resolved rules against native.

This isolates initialization + is-resolved or the nested terms schedule; it
is not the complete Pass 3.
The full schedule has a pre-existing device/native divergence and must be
reported separately from this native-checked scope.
"""
import argparse,hashlib,json
from pathlib import Path
from run import run
from compare_runs import compare

def verify(source,build,out,scope='resolved'):
    raw=source.read_text();lines=raw.splitlines(keepends=True)
    schedule=[i for i,line in enumerate(lines) if line.startswith('(run-schedule ')]
    # This extractor intentionally recognizes the saved flat Raytrace format,
    # rather than introducing another .egg parser or cutting arbitrary programs.
    if len(schedule)!=2 or any(line.strip() for line in lines[schedule[-1]+1:]) or schedule[1]!=schedule[0]+1:
        raise ValueError('expected the saved flat Raytrace source with two final schedule lines')
    if lines[schedule[0]].strip()!='(run-schedule (repeat 1 (run initialization)))':
        raise ValueError('unexpected initialization schedule')
    out.mkdir(parents=True,exist_ok=False)
    file=out/f'initial-{scope}.egg'
    closure='(saturate (run is-resolved))' if scope=='resolved' else '(saturate (run terms) (saturate (run terms-helpers) (saturate (run terms-helpers-helpers))))'
    file.write_text(''.join(lines[:schedule[0]+1])+'(run-schedule '+closure+')\n')
    paths=[out/'normal',out/'matrix']
    for mode,path in zip([False,True],paths):
        run(file,build,path,check_every=0,check_at_end=True,capacity=131072,match_capacity=131072,matrix_saturate=mode)
    result=compare(*paths,device_states=True,semantic_device=True,saturation_macros=scope=='terms')
    reports=[json.loads((p/'report.json').read_text()) for p in paths]
    fields=['set','rows','matched','updated','epoch','unions']
    histories=[[tuple(s[k] for k in fields) for s in r['steps']] for r in reports]
    if histories[0]!=histories[1] and scope=='resolved':
        raise AssertionError('logical Run history differs')
    result.update(scope='Real Raytrace initial graph and '+('17 is-resolved rules' if scope=='resolved' else 'all three nested terms rulesets')+'; isolated from the full schedule',
                  source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                  native_final_checks=[r['checks'][-1] for r in reports],
                  matrix_calls=reports[1]['matrix_saturate']['calls'],
                  logical_history_equal=histories[0]==histories[1])
    (out/'comparison.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k not in ['builds','matrix_calls']},indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',required=True,type=Path);p.add_argument('--build',required=True,type=Path);p.add_argument('--output',required=True,type=Path);p.add_argument('--scope',choices=['resolved','terms'],default='resolved');a=p.parse_args();verify(a.source,a.build,a.output,a.scope)

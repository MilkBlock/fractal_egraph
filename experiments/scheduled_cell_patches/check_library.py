"""Analyzer -> portable native control evidence -> fresh GPU runtime."""
import argparse,copy,json,os,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'experiments/device_backend'))
from run import run
from scheduled_matrix import validate_stage_evidence,bind_analyzer
from verify_native_source import verify

def main():
 p=argparse.ArgumentParser();p.add_argument('--analyzer',type=Path,default=ROOT/'target/debug/egg_layout');p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
 a.output.mkdir(parents=True,exist_ok=False);file=a.output/'source.egg'
 file.write_text('''(datatype X (N i64) (Edge X X)) (relation Seed (X)) (relation P (X)) (relation Q (X))
(ruleset a) (ruleset b)
(rule ((Seed x)) ((P x)) :ruleset a)
(rule ((Edge x y) (Q x)) ((P y)) :ruleset a)
(rule ((P x)) ((Q x)) :ruleset b)
(Seed (N 0)) (Edge (N 0) (N 1)) (Edge (N 1) (N 2))
(run-schedule (repeat 2 (run a) (saturate (run b)))) (check (Q (N 1)))
''')
 analysis=a.output/'analysis'
 with (a.output/'analyze.log').open('w') as log:
  subprocess.run([str(a.analyzer.resolve()),'analyze','--source',str(file.resolve()),'--output',str(analysis.resolve())],env=dict(os.environ,EGG_LAYOUT_GRAPHS='0'),stdout=log,stderr=subprocess.STDOUT,check=True)
 library=analysis/'cell-recipes.json';payload=json.loads(library.read_text())
 assert validate_stage_evidence(payload['source_schedule'],payload['stage_events'])==6
 for kind in ['truncated','wrong-position','wrong-progress']:
  bad=copy.deepcopy(payload['stage_events'])
  if kind=='truncated':bad.pop()
  elif kind=='wrong-position':bad[0]['iterations']=[9]
  else:bad[0]['can_stop']=True
  try:validate_stage_evidence(payload['source_schedule'],bad)
  except ValueError:pass
  else:raise AssertionError(kind+' was accepted')
 report=run(file,a.build,a.output/'gpu',schedule_library=library,check_at_end=True)
 assert report['scheduled_matrix']['program']['analyzer_evidence']['prepared_cells']>0
 assert sum(c['patch_operand_reads'] for c in report['scheduled_matrix']['calls'])>0
 check=verify(file,a.build,a.output/'gpu',a.output/'native')
 result={'native_schedule':check,'stage_events':6,'negative_control_tests':3,
         'analyzer':report['scheduled_matrix']['program']['analyzer_evidence'],'calls':report['scheduled_matrix']['calls']}
 (a.output/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
if __name__=='__main__':main()

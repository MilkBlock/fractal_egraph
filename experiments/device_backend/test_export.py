"""Native transport regression: set elements and frozen anchors survive union."""
import argparse,json,subprocess,tempfile
from pathlib import Path
SOURCE='''(sort E)
(constructor N (i64) E)
(sort S (Set E))
(constructor Box (S) E)
(ruleset r)
(let a (N 1))
(let b (N 2))
(Box (set-insert (set-insert (set-empty) a) b))
(rule ((= x (N 1)) (= y (N 2))) ((union x y)) :ruleset r)
(run r 1)
'''
def main():
 p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);a=p.parse_args()
 with tempfile.TemporaryDirectory() as d:
  source=Path(d)/'sets.egg';source.write_text(SOURCE)
  proc=subprocess.Popen([str(a.build.resolve()/'frontend'),str(source)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
  try:
   program=json.loads(proc.stdout.readline())
   box=next(t for t in program['tables'] if t['name']=='Box')
   assert box['schema'][0]['kind']=='set' and box['schema'][0]['element']['name']=='E'
   row=next(r for r in program['initial'] if r['table']=='Box')
   assert len(row['values'][0]['elements'])==2
   def request(q):
    proc.stdin.write(json.dumps(q)+'\n');proc.stdin.flush();return json.loads(proc.stdout.readline())
   request({'op':'run','set':'r'})
   state=request({'op':'snapshot'})
   row=next(r for r in state['rows'] if r['table']=='Box')
   assert len(row['values'][0]['elements'])==1
   for e in row['values'][0]['elements']:
    assert any(x['sort']==e['sort'] and x['now']==e['eq'] for x in state['anchors'])
   proc.stdin.write('{"op":"quit"}\n');proc.stdin.flush();assert proc.wait()==0
  finally:
   if proc.poll() is None:proc.kill();proc.wait()
 print('set export, canonical elements and frozen anchors passed')
if __name__=='__main__':main()

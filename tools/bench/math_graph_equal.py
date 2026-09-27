"""Exact correspondence for ground Math constructor graphs, modulo eclass IDs."""
import json
from pathlib import Path

def verify(left_path,right_path):
 left=json.loads(Path(left_path).read_text());right=json.loads(Path(right_path).read_text())
 index={(r['op'],tuple(map(tuple,r['args']))):r['result'] for r in right}
 assert len(index)==len(right)==len(left),'different enode counts or duplicate keys'
 mapping={};inverse={};pending=left[:]
 while pending:
  later=[]
  for r in pending:
   if any(t=='Math' and v not in mapping for t,v in r['args']):later.append(r);continue
   key=(r['op'],tuple((t,mapping[v] if t=='Math' else v) for t,v in r['args']))
   assert key in index,'missing constructor fact';target=index[key];source_id=r['result']
   assert source_id not in mapping or mapping[source_id]==target,'different equivalence'
   assert target not in inverse or inverse[target]==source_id,'non-bijective class map'
   mapping[source_id]=target;inverse[target]=source_id
  assert len(later)<len(pending),'unmapped cycle or ungrounded class'
  pending=later
 assert len(inverse)==len({r['result'] for r in right})
 return {'exact_equal':True,'enodes':len(left),'eclasses':len(mapping)}

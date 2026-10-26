"""Reuse a native-verified event transcript for a second GPU execution mode."""
import argparse
import hashlib
import json
from pathlib import Path
from dynamic_client import Resident

def replay(reference,worker,out,mode):
    r=json.loads((reference/'report.json').read_text())
    if not r['full_graph_check']['exact_rows_values_equivalence_visibility']:raise ValueError('unverified reference')
    library=json.loads((reference/'matrix/library.json').read_text());gpu=Resident(worker,library,out,mode)
    checks=0
    try:
        for command in r['commands']:
            info,payload=gpu.send(command['words'],allow_error=True)
            old=command['response']
            for key in ['ok','epoch','fact_count','live_contexts','context_capacity','runs_in_epoch','updated','new_facts_in_epoch']:
                if info.get(key)!=old.get(key):raise AssertionError((key,old,info))
            if hashlib.sha256(payload).hexdigest()!=command['payload_sha256']:raise AssertionError('dynamic state or visibility differs')
            if info.get('live_contexts')==0:
                assert info['context_reserved_bytes']==0 and info.get('forecast_reserved_bytes',0)==0
            checks+=1
        assert not gpu.commands,'normal resident clients must not accumulate retired command history'
        result={'mode':mode,'commands':checks,'exact_state_transcript':True,'native_reference':str(reference),
                'command_history_retained':len(gpu.commands),
                'scope':'correctness replay, not a timing comparison'}
        (out/'report.json').write_text(json.dumps(result,indent=2)+'\n');return result
    finally:gpu.close()

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--reference',type=Path,required=True);p.add_argument('--worker',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--mode',choices=['matrix','direct','local'],default='direct')
    a=p.parse_args();print(replay(a.reference,a.worker,a.output,a.mode))

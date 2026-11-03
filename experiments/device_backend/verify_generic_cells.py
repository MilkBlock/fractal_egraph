"""Strict native checks for loaded analyzer Cell programs, including Run order."""
import argparse,json
from pathlib import Path
from run import compare

def verify(output,references):
    results=[]
    for reference in references['references']:
        files=list(output.glob(f'program-*/cell-{reference["unit"]}/gpu-state.json'))
        if len(files)!=1:raise ValueError('missing/duplicate device instance')
        folder=files[0].parent;p=json.loads((folder/'program.json').read_text());s=json.loads(files[0].read_text())
        if not s.get('completed',s.get('published',False)):raise ValueError('incomplete private result is not a terminal state')
        check=compare(p,s['rows'],s['parents'],reference['checkpoint'],s['sets'])
        native=reference['events'];history=s['history']
        assert len(native)==len(history),'different local schedule length'
        for a,b in zip(native,history):
            assert (a['ruleset'],a['updated'],a['logical_matches'],a['after_rows'])==(p['sets'][b[0]],bool(b[1]),b[2],b[4]),'different local Run semantics'
        results.append(dict(unit=reference['unit'],runs=len(history),**check))
    report=dict(checked=len(results),checks=results,complete_state_and_each_run=True)
    (output/'verification.json').write_text(json.dumps(report,indent=2)+'\n');return report

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--reference',type=Path,required=True);a=p.parse_args()
    r=verify(a.output,json.loads(a.reference.read_text()));print(json.dumps({k:v for k,v in r.items() if k!='checks'},indent=2))

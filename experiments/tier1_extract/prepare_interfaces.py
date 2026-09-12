"""Retain the witnessed interface layout dropped by the compact template snapshot.
No rule composition here: only copy existing occurrence IDs, parents and slot schemas.
"""
import argparse,json,hashlib
from collections import defaultdict
from pathlib import Path
OUT=Path(__file__).parent
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('manifest');p.add_argument('native');p.add_argument('profile');a=p.parse_args()
    m=json.loads(Path(a.manifest).read_text()); n=json.loads(Path(a.native).read_text()); profile=json.loads(Path(a.profile).read_text())
    saved=json.loads((OUT/'native_templates.json').read_text())
    assert saved['native_egraph']==n['native_egraph'], 'must use the same native tier-1 graph'
    instances={i['event']:i['template'] for i in n['instances']}
    reads=defaultdict(list)
    for r in profile['reads']:reads[r['match']].append(r)
    records=[]
    for r in m['records']:
        named=[x.removeprefix('variable:') for x in r['output_layout'] if x.startswith('variable:')]
        rr=sorted(reads[r['id']],key=lambda x:(x.get('source_span') or '',x['name'] or '',x['id']))
        records.append({'event':r['id'],'template':instances[r['id']], 'parents':r['parents'],
            'inputs':[{'variable':v} for v in named]+[{'read_span':x['source_span'],'op':x['name']} for x in rr],
            'outputs':r['output_layout']})
    result={'scope':'Interface side table for the exact saved native tier-1 graph. No concrete values or candidate composition.',
        'native_sha256':hashlib.sha256((OUT/'native_templates.json').read_bytes()).hexdigest(),'occurrences':records}
    (OUT/'interfaces.json').write_text(json.dumps(result,indent=2)+'\n')

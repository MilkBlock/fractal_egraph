"""Convert completed training-only native observations to coverage pattern inputs.
Boundary markers become slot incidences; no unfinished/rejected state is admitted.
"""
import argparse,hashlib,json,pathlib
p=argparse.ArgumentParser();p.add_argument('--analysis',type=pathlib.Path);p.add_argument('--output',type=pathlib.Path,required=True);p.add_argument('--merge-old',type=pathlib.Path);p.add_argument('--merge-new',type=pathlib.Path);a=p.parse_args()
if a.merge_old or a.merge_new:
    if not (a.merge_old and a.merge_new) or a.analysis:p.error('provide both merge libraries, without --analysis')
    old=json.loads(a.merge_old.read_text());new=json.loads(a.merge_new.read_text())
    if old['version']!=1 or new['version']!=1:raise ValueError('unsupported library version')
    start=max((x['id'] for x in old['patterns']),default=-1)+1
    for i,pattern in enumerate(new['patterns']):pattern['id']=start+i
    old['patterns'].extend(new['patterns']);old['signatures']=[]
    old['scope']='Training-only union of previous empirical patterns and newly completed observations; no certified transitions'
    old['unsupported'].extend(new['unsupported'])
    with a.output.open('x') as file:file.write(json.dumps(old,separators=(',',':'))+'\n')
    print('merged',len(old['patterns']),'pattern occurrences; signatures reset for training')
    raise SystemExit()
if not a.analysis:p.error('--analysis is required for observation conversion')
a.output.mkdir(parents=True,exist_ok=False)
report=json.loads((a.analysis/'analysis.json').read_text());jobs=report['saturated_rule_composition']['jobs'];entries=[]
for j in jobs:
    if j['state']!='Saturated':continue
    file=j.get('export',{}).get('observed_state',{}).get('file')
    if not file:continue
    path=pathlib.Path(j['folder'])/file
    raw=path.read_bytes();state=json.loads(raw)
    assert state['scope'].get('observation_only') is True
    assert not state.get('ports'), 'named interface ports require explicit coverage handling'
    markers=set(j['origin']['parameter_markers'].values())
    rows=[];hidden=[];boundaries=[]
    for i,row in enumerate(state['rows']):
        if row['op'] in markers:
            if len(row['args'])!=1:raise ValueError('invalid boundary marker')
            boundaries.append(row['result']);continue
        if i in state.get('subsumed_rows',[]):hidden.append(len(rows))
        rows.append(row)
    for value in boundaries:
        slot=len(state['values']);state['values'].append({'sort':'@input:'+state['values'][value]['sort'],'literal':None})
        rows.append({'op':'@binding','args':[value],'result':slot})
    state.update(rows=rows,subsumed_rows=hidden,local_ids=[],ports={})
    target=f"group-{j['candidate_id']:06}.json"
    (a.output/target).write_text(json.dumps(state,separators=(',',':'))+'\n')
    entries.append({'file':target,'job':j['candidate_id'],'source_sha256':hashlib.sha256(raw).hexdigest(),'boundary_slots':len(boundaries)})
(a.output/'manifest.json').write_text(json.dumps({'source_sha256':hashlib.sha256((a.analysis/'source.egg').read_bytes()).hexdigest(),
    'scope':'Completed observed training states, not certified transitions; duplicate occurrences retained and reported','states':len(entries),'entries':entries},indent=2)+'\n')
print('converted',len(entries),'completed observations')

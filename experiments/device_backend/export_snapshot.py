"""Load a GPU/native typed snapshot into CPU extraction tables, without inference."""
import argparse,collections,json,subprocess
from pathlib import Path

def gpu_rows(program,state):
    parents=state['parents'];arena=state['sets'];sorts={};valid_sets=set();h=2
    if state['meta'][4] or len(state['rows'])!=state['meta'][0]:raise ValueError('incomplete GPU state')
    while h<arena[0]:
        if h+4>arena[0] or not 0<=arena[h+1]<=arena[h]:raise ValueError('invalid set record')
        valid_sets.add(h);h+=arena[h]+4
    if h!=arena[0] or h>len(arena):raise ValueError('invalid set arena')
    set_names=sorted({s['name'] for t in program['tables'] for s in t['schema'] if s['kind']=='set'})
    def value(s,x):
        if s['kind']=='structural_key':
            from eggfuse.ir.structural_key import arena_key
            key_arena=state.get('structural_keys')
            if key_arena is None:
                offset=program['structural_key']['offset_words'];key_arena=arena[offset:offset+arena[offset]]
            return {'sort':s['name'],'key':arena_key(key_arena,x)}
        if s['kind']=='eq':
            if not 0<x<len(parents) or parents[x]!=x:raise ValueError('noncanonical equality value')
            if sorts.setdefault(x,s['name'])!=s['name']:raise ValueError('cross-sort equality')
            return {'sort':s['name'],'eq':f'gpu:{x}'}
        if s['kind']=='set':
            if x not in valid_sets or arena[x+2]!=set_names.index(s['name']):raise ValueError('invalid set identity')
            es=arena[x+4:x+4+arena[x+1]]
            if es!=sorted(set(es)):raise ValueError('noncanonical set members')
            return {'sort':s['name'],'elements':[value(s['element'],e) for e in es]}
        if s['kind']=='String':
            if not 0<=x<len(program['strings']):raise ValueError('invalid string handle')
            x=program['strings'][x]
        return {'sort':s['name'],'value':x}
    rows=[]
    for row in state['rows']:
        if len(row)!=program['stride'] or not 0<=row[0]<len(program['tables']) or row[1] not in (0,1):raise ValueError('invalid row header')
        t=program['tables'][row[0]]
        rows.append({'table':t['name'],'subsumed':bool(row[1]),'values':[value(s,v) for s,v in zip(t['schema'],row[4:])]})
    return rows

def verify_import(rows,imported):
    mapping={(x['sort'],x['old']):x['now'] for x in imported['identities']}
    def v(x,remap):
        if 'key' in x:
            from eggfuse.ir.structural_key import key_token
            return (x['sort'],'key',key_token(x['key']))
        if 'eq' in x:return (x['sort'],'eq',mapping[(x['sort'],x['eq'])] if remap else x['eq'])
        if 'elements' in x:return (x['sort'],'set',frozenset(v(e,remap) for e in x['elements']))
        return (x['sort'],'value',x['value'])
    def r(x,remap):return (x['table'],x['subsumed'],tuple(v(e,remap) for e in x['values']))
    if collections.Counter(r(x,True) for x in rows)!=collections.Counter(r(x,False) for x in imported['rows']):raise AssertionError('native import changed the snapshot')
    if imported['rules_executed'] or imported['rebuilds_executed']:raise AssertionError('import ran inference')
    return {'exact_import':True,'rows':len(rows),'classes':len(mapping),'rules_executed':0,'rebuilds_executed':0}

def export(source,build,run,lane,output,extract_table=None):
    output.mkdir(parents=True,exist_ok=False)
    if lane=='gpu':rows=gpu_rows(json.loads((run/'program.json').read_text()),json.loads((run/'gpu-state.json').read_text()))
    else:rows=json.loads((run/'native-state.json').read_text())['rows']
    incoming=output/'rows.json';incoming.write_text(json.dumps(rows))
    native=output/'native'
    cmd=[str((build/'frontend').resolve()),str(source.resolve()),'--load-rows',str(incoming),'--export-to',str(native)]
    if extract_table:cmd+=['--extract-table',extract_table]
    with (output/'stderr.log').open('w') as log:status=json.loads(subprocess.check_output(cmd,stderr=log,text=True))
    imported=json.loads((native/'import.json').read_text());result=verify_import(rows,imported)
    result.update(lane=lane,extracted=status['extracted'],graph=str(native/'graph.json'),scope='CPU extraction input only; import fidelity is not CPU/GPU inference equality')
    (output/'report.json').write_text(json.dumps(result,indent=2)+'\n')
    return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--build',type=Path,required=True);p.add_argument('--run',type=Path,required=True);p.add_argument('--lane',choices=['gpu','native'],default='gpu');p.add_argument('--output',type=Path,required=True);p.add_argument('--extract-table');a=p.parse_args()
    print(json.dumps(export(a.source,a.build,a.run,a.lane,a.output,a.extract_table),indent=2))

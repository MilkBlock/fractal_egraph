"""Real GPU/native stage isolation, executable Bake loading and exact patches."""
import argparse,copy,hashlib,json,struct
from pathlib import Path
from run import run
from compare_runs import compare
from stage_cell import digest,load
from test_joint_polyspine import BASE,SCHEDULE,rejected_transaction

def words_to_rows(words,stride):
    data=struct.pack('<'+'I'*len(words),*words)
    values=struct.unpack('<'+'q'*(len(data)//8),data)
    return [values[i:i+stride] for i in range(0,len(values),stride)]

def root(parents,x):
    seen=set()
    while parents[x]!=x:
        assert x not in seen;seen.add(x);x=parents[x]
    return x

def audit_export(directory):
    program=json.loads((directory/'program.json').read_text());tables=program['tables'];checked=0;different_equalities=0;late_functions=0
    for file in (directory/'stage-contexts').glob('scope-*.json'):
        data=json.loads(file.read_text());views=data['views'];byid={v['version']:v for v in views+[data['result']]}
        for p in data['patches']:
            before=byid[p['from']];after={}
            for name,d in p['components'].items():
                n=d['extent'];a=(before['parts'][name][:n]+[0]*max(0,n-len(before['parts'][name])))
                offsets=set()
                for off,value in zip(d['offset_values'][::2],d['offset_values'][1::2]):
                    assert off<n and off not in offsets;offsets.add(off);a[off]=value
                after[name]=a
            if p['to'] in byid:assert after==byid[p['to']]['parts'];checked+=1
        for index,view in enumerate(views):
            counts=[0]*len(tables)
            for row in words_to_rows(view['parts']['rows'],program['stride']):
                if row[1]==0 and tables[row[0]]['name'] in data['scan_read_tables'][index]:counts[row[0]]+=1
            assert counts==data['scan_table_counts'][index]
        q=list(zip(*[iter(data['queries'])]*6));answers=list(zip(*[iter(data['answers'])]*4))
        for request,answer in zip(q,answers):
            slot,version,kind,a,b,nt=request;view=views[slot];parents=view['parts']['parents']
            if version!=view['version']:assert answer[0]==1;continue
            assert answer[0]==0
            if kind==0:
                x,y=root(parents,a),root(parents,b);assert answer[1:]==(int(x==y),x,y)
                if slot==0 and x!=y:different_equalities+=1
            else:
                rows=[r for r in words_to_rows(view['parts']['rows'],program['stride']) if r[0]==a and r[1]==0 and root(parents,r[4])==root(parents,b)]
                assert answer[1]==bool(rows)
                if rows:
                    schema=tables[a]['schema'];value=rows[0][5];value=root(parents,value) if schema[-1]['kind']=='eq' else value
                    assert answer[2]==value
                elif slot==0:late_functions+=1
    assert checked>0
    return {'patches_reconstructed':checked,'old_context_unequal_pairs':different_equalities,'old_context_missing_functions':late_functions}

def tests(build,out):
    out.mkdir(parents=True,exist_ok=False);results={};library=out/'terms.cell.json'
    source=out/'bake.egg';source.write_text(BASE+'''(let x (Num 1)) (let y (Num 2)) (Add x y)
'''+SCHEDULE)
    baked=run(source,build,out/'bake',bake_stage_cell=library,stage_context_export=True)
    assert baked['stage_contexts']['patches']>3 and baked['checks'][-1]['exact_rows_values_equivalence_visibility']
    artifact=json.loads(library.read_text());assert len(artifact['contract']['stages'])==3
    assert 'Smaller' in artifact['contract']['stages'][1]['writes'],'merge callback footprint lost'
    assert not any(t['name'] in ['x','y'] for t in artifact['contract']['tables'])
    results['bake']=audit_export(out/'bake')
    cases={
        'new-bindings':BASE+'(let a (Num 8)) (let b (Num 9)) (let t (Triple (Add a b) a b))\n'+SCHEDULE+'(check (= (ExtractedExpr t) (TCPair (TermTriple (TermAdd (TermNum 8) (TermNum 9)) (TermNum 8) (TermNum 9)) 6)))',
        'pending-union':BASE+'''(let x (External 7)) (let a (TCPair (TermNum 10) 1)) (let b (TCPair (TermNum 20) 4))
(let s (Smaller a b)) (set (ExtractedExpr x) s) (Add x (Num 9))
'''+SCHEDULE,
        'correlated-aliases':BASE+'''(let x (External 1)) (let y (External 2))
(let a (TCPair (TermNum 10) 1)) (union a (TCPair (TermNum 11) 5))
(set (ExtractedExpr x) a) (set (ExtractedExpr y) (TCPair (TermNum 20) 2)) (Add x y) (Add x x)
'''+SCHEDULE,
        'large-sentinel':BASE+'''(let a (Num 1)) (let b (Num 2)) (Add a b)
(run terms 1) (run terms-helpers 1)
'''+SCHEDULE+'(check (PotentialExtractedExpr (Add a b) (TCPair (TermAdd (NoTerm) (NoTerm)) 20000000000000001)))',
        'multiple-spines':BASE+'''(let a (Num 1)) (let b (Num 2)) (union (Add a b) (Triple a b a))
'''+SCHEDULE,
        'external-union-retirement':BASE+'''(let a (Num 1)) (let b (Num 2)) (Add a b)
(ruleset outside) (rule ((= a (Num 1)) (= b (Num 2))) ((union a b) (subsume (Add a b))) :ruleset outside)
'''+SCHEDULE+'(run outside 1)\n'+SCHEDULE,
        'finite-run':BASE+'(Add (Num 1) (Num 2)) (run terms 1) (run terms-helpers 1) (run terms-helpers-helpers 1)',
    }
    for name,text in cases.items():
        file=out/(name+'.egg');file.write_text(text)
        baseline=run(file,build,out/(name+'-ordinary'),matrix_saturate=True)
        candidate=run(file,build,out/(name+'-stages'),stage_cell=library,stage_context_audit=True,stage_context_export=True,matrix_saturate=True)
        assert candidate['checks'][-1]['exact_rows_values_equivalence_visibility']
        if name=='finite-run':assert candidate['stage_contexts']['scope_count']==0;results[name]=True;continue
        assert all(c['accepted'] for c in candidate['joint_polyspine']['calls'])
        for scope in candidate['stage_contexts']['scopes']:
            assert scope['wrong_base_patch_rejected']
            assert all(a['output_version']==b['input_version'] for a,b in zip(scope['history'],scope['history'][1:]))
        check=compare(out/(name+'-ordinary'),out/(name+'-stages'),device_states=True,semantic_device=True,saturation_macros=True)
        results[name]={'graph':check['final_check'],'contexts':audit_export(out/(name+'-stages'))}
        print(name,'passed',flush=True)
    assert results['pending-union']['contexts']['old_context_unequal_pairs']>0
    assert results['new-bindings']['contexts']['old_context_missing_functions']>0
    wrong=copy.deepcopy(artifact);wrong['contract']['stages'][0]['reads']=[];wrong['contract_sha256']=digest(wrong['contract']);bad=out/'wrong.cell.json';bad.write_text(json.dumps(wrong))
    try:load(bad,{k:artifact[k] for k in ['contract','contract_sha256']})
    except ValueError as e:assert 'differ' in str(e)
    else:raise AssertionError('modified stage contract accepted')
    results['changed-contract-rejected']=True
    resumed=run(out/'new-bindings.egg',build,out/'resumed',stage_cell=library,resume=out/'new-bindings-stages')
    assert resumed['complete_schedule'] and resumed['checks'][-1]['exact_rows_values_equivalence_visibility'];results['resume']=True
    for name,text,code in [
        ('overflow',BASE+'(let x (External 1)) (set (ExtractedExpr x) (TCPair (TermNum 1) 9223372036854775807)) (Add x x)'+SCHEDULE,5),
        ('panic',BASE+'(PotentialExtractedExpr (External 1) (TCPair (TermNum 1) -1))'+SCHEDULE,8)]:
        file=out/(name+'.egg');file.write_text(text)
        results[name+'-stage-rollback']=rejected_transaction(build,file,out/(name+'-transaction'),code,stage_contexts=True)
    (out/'results.json').write_text(json.dumps(results,indent=2)+'\n');return results

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();r=tests(a.build,a.output);print('passed',len(r),'multi-stage checks')

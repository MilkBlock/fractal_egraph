"""CPU import/extraction and narrow selected-DAG equivalence regressions."""
import argparse,json,shutil,subprocess
from pathlib import Path
from export_snapshot import export,gpu_rows
from selected_dag import compare_programs
from compare_runs import compare as compare_devices
HERE=Path(__file__).resolve().parent

def main():
    a=argparse.ArgumentParser();a.add_argument('--build',type=Path,required=True);a.add_argument('--runs',type=Path,required=True);a.add_argument('--output',type=Path,required=True);a=a.parse_args()
    a.output.mkdir(parents=True,exist_ok=False);results={}
    for name,source,table in [('math',HERE/'fixtures/math.egg','Add'),('sets',HERE/'fixtures/sets.egg',None),('pipeline',HERE/'pipeline.egg',None),('custom-merge',HERE/'fixtures/custom-merge.egg',None),('float-identity',HERE/'fixtures/float-identity.egg',None)]:
        results[name]=export(source,a.build,a.runs/name,'gpu',a.output/name,table)
    assert results['math']['extracted'][0]['term']=='(Num 1)'
    p=json.loads((a.runs/'math/program.json').read_text());s=json.loads((a.runs/'math/gpu-state.json').read_text());rows=gpu_rows(p,s);rows.append(rows[0])
    bad=a.output/'duplicate.json';bad.write_text(json.dumps(rows))
    result=subprocess.run([str(a.build.resolve()/'frontend'),str(HERE/'fixtures/math.egg'),'--load-rows',str(bad),'--export-to',str(a.output/'bad-import')],capture_output=True,text=True)
    assert result.returncode and 'duplicate canonical snapshot key' in result.stderr
    results['reject-duplicate-without-repair']=True
    def selected(name,body):
        path=a.output/(name+'.egg')
        path.write_text('(datatype Expr (N i64) (Concat Expr Expr) (Function String Expr))\n(Function "f" '+body+')\n')
        return json.loads(subprocess.check_output([str(a.build.resolve()/'frontend'),str(path),'--selected-dag'],text=True))
    left=selected('left','(Concat (Concat (N 1) (N 2)) (N 3))')
    right=selected('right','(Concat (N 1) (Concat (N 2) (N 3)))')
    wrong=selected('wrong','(Concat (N 1) (Concat (N 3) (N 2)))')
    eq=compare_programs(left,right);assert not eq['exact_roots']['equal'] and eq['concat_associative_roots']['equal'];results['concat-association']=eq
    assert not compare_programs(left,wrong)['concat_associative_roots']['equal'];results['reject-reordered-elements']=True
    original=a.runs/'math';changed=a.output/'device-padding';changed.mkdir()
    for name in ['report.json','program.json','core.json','gpu-state.json']:
        shutil.copyfile(original/name,changed/name)
    p=json.loads((changed/'program.json').read_text());s=json.loads((changed/'gpu-state.json').read_text())
    row=next(row for row in s['rows'] if 4+len(p['tables'][row[0]]['schema'])<len(row))
    row[-1]=123456
    (changed/'gpu-state.json').write_text(json.dumps(s))
    check=compare_devices(original,changed,True)['final_check']
    assert not check['exact_device_state'] and check['exact_rows_values_equivalence_visibility']
    results['ignore-only-unused-device-padding']=True
    row[1]=1-row[1]
    (changed/'gpu-state.json').write_text(json.dumps(s))
    try:compare_devices(original,changed,True)
    except ValueError as error:assert 'declared row fields' in str(error)
    else:raise AssertionError('ignored a visibility difference with the padding')
    results['reject-declared-device-field-change']=True
    (a.output/'results.json').write_text(json.dumps(results,indent=2)+'\n');print('passed',len(results),'snapshot/extraction checks')
if __name__=='__main__':main()

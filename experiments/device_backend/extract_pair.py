"""Production Tiger comparison for two GPU-owned runtime endpoints.

Reuse the existing exact snapshot importer and strict selected-DAG checker.
Import/extraction is diagnostic and outside inference timing; neither lane is
labelled native inference. The supplied same-input graph comparison must pass.
"""
import argparse,hashlib,json,os,shutil,subprocess,time
from pathlib import Path
from compare_runs import compare
from export_snapshot import export,gpu_rows

def extract_pair(source,build,before,after,tiger,checker,output,reuse_control=None):
    output.mkdir(parents=True,exist_ok=False)
    graph=compare(before,after,device_states=True,semantic_device=True,saturation_macros=True)
    (output/'graph-check.json').write_text(json.dumps(graph,indent=2)+'\n')
    sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
    report={'scope':'production Tiger on exact GPU-off/GPU-on endpoints; not native CPU inference equivalence',
            'tiger_sha256':sha(tiger),'checker_sha256':sha(checker),'lanes':{}}
    env={k:v for k,v in os.environ.items() if not k.startswith('TIGER_EXTRACTION_')};env['TIGER_EXTRACTION_MARKERS']='off'
    for lane,run in [('control',before),('updates',after)]:
        if lane=='control' and reuse_control is not None:
            cached=json.loads((reuse_control/'report.json').read_text())
            cache_graph=json.loads((reuse_control/'graph-check.json').read_text())
            cache_lane=next((label for label,key in [('control','before'),('updates','after')] if Path(cache_graph[key]).resolve()==run.resolve()),None)
            if cached['tiger_sha256']!=sha(tiger) or cache_lane is None or cache_graph['source_sha256']!=graph['source_sha256']:raise ValueError('cached extraction control/tool/source differs')
            receipt=cached['lanes'][cache_lane];egg=reuse_control/(cache_lane+'.egg')
            if receipt['output_sha256']!=sha(egg):raise ValueError('cached extraction output changed')
            rows=gpu_rows(json.loads((run/'program.json').read_text()),json.loads((run/'gpu-state.json').read_text()))
            if rows!=json.loads((reuse_control/cache_lane/'rows.json').read_text()):raise ValueError('cached extraction input changed')
            if not receipt['import']['exact_import']:raise ValueError('cached import was not verified')
            shutil.copyfile(egg,output/'control.egg');report['lanes'][lane]={**receipt,'reused_from':str(reuse_control)};continue
        result=export(source,build,run,'gpu',output/lane)
        # Tiger's production reader expects pretty JSON with whitespace.
        raw=Path(result['graph']);pretty=output/(lane+'.json');pretty.write_text(json.dumps(json.loads(raw.read_text()),indent=2)+'\n')
        with pretty.open('rb') as inp,(output/(lane+'.egg')).open('wb') as out,(output/(lane+'.log')).open('wb') as err:
            start=time.perf_counter();subprocess.run([str(tiger.resolve()),'--ilp-solver','cbc'],stdin=inp,stdout=out,stderr=err,env=env,check=True);elapsed=time.perf_counter()-start
        report['lanes'][lane]={'import':result,'seconds':elapsed,'output_sha256':sha(output/(lane+'.egg'))}
        pretty.unlink()
    subprocess.run([str(checker.resolve()),str(output/'quality.json'),str(output/'control.egg'),str(output/'updates.egg')],check=True)
    report['quality']=json.loads((output/'quality.json').read_text());(output/'report.json').write_text(json.dumps(report,indent=2)+'\n');return report

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--build',type=Path,required=True);p.add_argument('--before',type=Path,required=True);p.add_argument('--after',type=Path,required=True);p.add_argument('--tiger',type=Path,required=True);p.add_argument('--checker',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--reuse-control',type=Path);a=p.parse_args()
    r=extract_pair(a.source,a.build,a.before,a.after,a.tiger,a.checker,a.output,a.reuse_control);print('strict selected DAG:',r['quality']['passed'])

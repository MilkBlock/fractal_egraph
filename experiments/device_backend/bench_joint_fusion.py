"""Single paired full-schedule run; keep joint contracts and effects identical."""
import argparse,json,os
from pathlib import Path
from run import run
from compare_runs import compare

def bench(source,build,output,marker,cell,capacity,match_capacity,optimization="dispatch"):
 output.mkdir(parents=True,exist_ok=False);reports={}
 for mode in ['off','on']:
  os.environ['EGGFUSE_JOINT_FUSE_PREPARE']=os.environ['EGGFUSE_JOINT_LOCAL_SORT']='1' if mode=='on' and optimization in ['dispatch','all'] else '0'
  os.environ['EGGFUSE_JOINT_SHARED_ENCODER']='1' if mode=='on' and optimization in ['encoder','all'] else '0'
  os.environ['EGGFUSE_SHARED_ENCODER']='1' if mode=='on' and optimization in ['backend','all'] else '0'
  r=run(source,build,output/mode,check_every=0,capacity=capacity,match_capacity=match_capacity,
        save_final_state=True,marker_program=marker,cell_program=cell,stage_contexts=True,matrix_saturate=True)
  assert r['complete_schedule'] and r['clean_timing'] and r['joint_polyspine']['calls']
  reports[mode]=r;print(mode,r['inference_seconds'],r['joint_execution'],flush=True)
 check=compare(output/'off',output/'on',device_states=True,semantic_device=True,saturation_macros=True)
 from eggfuse.compiler.multistage import execution_summary
 result=dict(scope='actual complete source schedule; same binary on/off; not pristine native equivalence',
             optimization=optimization,source_sha256=reports['on']['source_sha256'],worker_sha256=reports['on']['build']['worker_sha256'],
             graph=check['final_check'],lanes={})
 for mode,r in reports.items():result['lanes'][mode]=dict(inference_seconds=r['inference_seconds'],setup_seconds=r['setup_seconds'],joint=execution_summary(r),execution=r['joint_execution'],shared_run_encoding=r.get('shared_run_encoding'),timings=r['timings_seconds'])
 result['speedup']=reports['off']['inference_seconds']/reports['on']['inference_seconds']
 result['reduction_percent']=100*(1-1/result['speedup'])
 (output/'comparison.json').write_text(json.dumps(result,indent=2)+'\n');return result
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--marker',type=Path,required=True);p.add_argument('--cell',type=Path,required=True);p.add_argument('--optimization',choices=['dispatch','encoder','backend','all'],default='dispatch');p.add_argument('--capacity',type=int,default=1048576);p.add_argument('--match-capacity',type=int,default=1048576);a=p.parse_args();r=bench(a.source,a.build,a.output,a.marker,a.cell,a.capacity,a.match_capacity,a.optimization);print('full inference speedup',r['speedup'],flush=True)

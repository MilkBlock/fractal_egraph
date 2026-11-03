"""Single-pair throughput check. Synthetic stage family; never called Pass 3."""
import argparse,hashlib,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'experiments/device_backend'))
from run import run
from compare_runs import compare
from verify_native_source import verify

def main():
 p=argparse.ArgumentParser();p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--instances',type=int,default=8192);p.add_argument('--stages',type=int,default=28);a=p.parse_args()
 a.output.mkdir(parents=True,exist_ok=False)
 n,k=a.instances,a.stages
 if n<1 or k<2:raise ValueError('positive instance count, at least two stages')
 source='(datatype X (N i64))\n'+''.join(f'(relation P{i} (X)) (ruleset s{i})\n' for i in range(k))
 source+='(rule ((= x (N n))) ((P0 x)) :ruleset s0)\n'
 source+=''.join(f'(rule ((P{i-1} x)) ((P{i} x)) :ruleset s{i})\n' for i in range(1,k))
 source+=''.join(f'(N {i})\n' for i in range(n))
 source+='(run-schedule (repeat 5 '+''.join(f'(run s{i}) ' for i in range(k))+'))\n'
 file=a.output/'source.egg';file.write_text(source);capacity=1<<((n*(k+1)+16).bit_length())
 before=a.output/'ordinary';after=a.output/'patch'
 normal=run(file,a.build,before,capacity=capacity,match_capacity=max(16384,n*2),check_at_end=True)
 patch=run(file,a.build,after,capacity=capacity,match_capacity=max(16384,n*2),scheduled_matrix=True,verify_against=before)
 original=verify(file,a.build,after,a.output/'original-native')
 check=compare(before,after,device_states=True,semantic_device=True)
 result={'scope':'actual GPU runtime, synthetic fixed-carrier multi-stage family; not Raytrace coverage',
         'instances':n,'stages':k,'source_sha256':hashlib.sha256(file.read_bytes()).hexdigest(),
         'worker_sha256':patch['build']['worker_sha256'],'same_binary':normal['build']['worker_sha256']==patch['build']['worker_sha256'],
         'ordinary_seconds':normal['inference_seconds'],'patch_seconds':patch['inference_seconds'],
         'speedup':normal['inference_seconds']/patch['inference_seconds'],'gpu_phase_times':check['timings'],
         'graph':check['final_check'],'calls':patch['scheduled_matrix']['calls'],
         'original_native_schedule':original,'native_checked':patch['checks'][-1]['exact_rows_values_equivalence_visibility']}
 (a.output/'results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
if __name__=='__main__':main()

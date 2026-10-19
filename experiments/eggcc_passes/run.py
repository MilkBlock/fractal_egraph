"""One actual three-pass run; optional separate coverage or schedule timing only."""
import argparse,os,pathlib,subprocess,time,json,hashlib,sys
p=argparse.ArgumentParser();p.add_argument('--output',type=pathlib.Path,required=True);p.add_argument('--library',type=pathlib.Path);p.add_argument('--profile-binary',type=pathlib.Path,default=pathlib.Path('out/eggcc-pass-audit/eggcc-profile'));p.add_argument('--source',type=pathlib.Path,action='append',help='Workload source; repeat for multiple workloads');p.add_argument('--timing-only',action='store_true',help='Skip coverage replay; summarize schedule-profile output when available');p.add_argument('--production-repo',type=pathlib.Path);a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];repo=root.parent/'check_statewalk'
repo=(a.production_repo or repo).resolve()
if not a.timing_only and a.library is None:p.error('--library is required for coverage audits')
assert not a.output.exists(),'use a fresh output directory'
a.output.mkdir(parents=True)
sources=a.source or [repo/f'benchmarks/passing/bril/core/{name}.bril' for name in ['fact','loopfact']]
for source_input in sources:
 name=source_input.stem
 folder=a.output/name;folder.mkdir()
 start=time.perf_counter()
 with (folder/'program.txt').open('w') as stdout,(folder/'profile.log').open('w') as stderr:
  subprocess.run([str(a.profile_binary.resolve()),str(source_input.resolve()),'--run-mode','optimized-pretty-print','--eggcc-schedule','parallel','--run-data-out',str(folder/'profile-run.json')],env=dict(os.environ,EGGCC_PASS_PROFILE_DIR=str(folder.resolve())),stdout=stdout,stderr=stderr,check=True)
 elapsed=time.perf_counter()-start
 (folder/'measurement.json').write_text(json.dumps({'source':str(source_input.resolve()),'source_sha256':hashlib.sha256(source_input.read_bytes()).hexdigest(),'binary_sha256':hashlib.sha256(a.profile_binary.read_bytes()).hexdigest(),'cli_wall_seconds':elapsed,'timing_only':a.timing_only,'production_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()},indent=2)+'\n')
 if a.timing_only:continue
 for i in range(1,4):
  source=folder/f'pass-{i}-flat.egg'
  subprocess.run([str(repo/'target/release/flatten'),str(folder/f'pass-{i}.egg'),str(source)],check=True)
  with (folder/f'coverage-stage{i}.log').open('w') as log:
   subprocess.run([str(root/'target/release/examples/ripsat_coverage'),'eval',str(source),str(a.library),str(folder/f'coverage-stage{i}')],stdout=log,stderr=log,check=True)
if a.timing_only:
 from saturation_report import summarize
 summarize(a.output)
else:
 subprocess.run([sys.executable,str(root/'experiments/eggcc_passes/summarize.py'),'--input',str(a.output),'--library',str(a.library),'--output',str(a.output/'summary.json')],check=True)

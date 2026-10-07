"""One actual three-pass run per workload, followed by separate coverage audits."""
import argparse,os,pathlib,subprocess
p=argparse.ArgumentParser();p.add_argument('--output',type=pathlib.Path,required=True);p.add_argument('--library',type=pathlib.Path,required=True);p.add_argument('--profile-binary',type=pathlib.Path,default=pathlib.Path('out/eggcc-pass-audit/eggcc-profile'));a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];repo=root.parent/'check_statewalk'
assert not a.output.exists(),'use a fresh output directory'
a.output.mkdir(parents=True)
for name in ['fact','loopfact']:
 folder=a.output/name;folder.mkdir()
 with (folder/'program.txt').open('w') as stdout,(folder/'profile.log').open('w') as stderr:
  subprocess.run([str(a.profile_binary.resolve()),str(repo/f'benchmarks/passing/bril/core/{name}.bril'),'--run-mode','dag-optimize','--eggcc-schedule','parallel','--run-data-out',str(folder/'profile-run.json')],env=dict(os.environ,EGGCC_PASS_PROFILE_DIR=str(folder.resolve())),stdout=stdout,stderr=stderr,check=True)
 for i in range(1,4):
  source=folder/f'pass-{i}-flat.egg'
  subprocess.run([str(repo/'target/release/flatten'),str(folder/f'pass-{i}.egg'),str(source)],check=True)
  with (folder/f'coverage-stage{i}.log').open('w') as log:
   subprocess.run([str(root/'target/release/examples/ripsat_coverage'),'eval',str(source),str(a.library),str(folder/f'coverage-stage{i}')],stdout=log,stderr=log,check=True)
subprocess.run(['python3',str(root/'experiments/eggcc_passes/summarize.py'),'--input',str(a.output),'--library',str(a.library),'--output',str(a.output/'summary.json')],check=True)

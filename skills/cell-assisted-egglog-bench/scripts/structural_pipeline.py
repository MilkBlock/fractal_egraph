"""Explicit source -> standalone structural keys -> source-level markers."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def run(a,repo):
    resolve=lambda p:p if p.is_absolute() else repo/p
    source=resolve(a.source);output=resolve(a.output)
    if output.exists():raise ValueError('use a fresh pipeline output')
    python=resolve(a.solver_python) if a.solver_python else next((p for p in [repo/'out/cell-spine-venv/bin/python',Path('/Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python')] if p.is_file()),Path(sys.executable))
    commands=[]
    if not a.no_build:
        commands += [('build',['cargo','build','--offline','--release','--target-dir','target/analyzer','--example','structural_key_source','--example','lhs_ast','--example','scheduled_spine'])]
    if not a.no_build:
        commands += [('build-profiler',['cargo','build','--offline','--release','--target-dir','target/analyzer','--features','planner-profile','--example','planner_profile'])]
    commands += [('structural-keys',[str(python),'-m','tools.cell_spine.structural_keys','--source',str(source),'--output',str(output/'sk')]),
        ('markers',[str(python),'-m','tools.cell_spine.source_pipeline','--source',str(output/'sk/source_sk.egg'),'--output',str(output/'mk'),'--solver',a.solver])]
    if a.check:
        commands.append(('comparison',[sys.executable,str(repo/'skills/cell-assisted-egglog-bench/scripts/bench.py'),'--repo',str(repo),'program-modes',
            '--source',str(output/'source_sk.egg'),'--marked',str(output/'source_sk_mk.egg'),'--output',str(output/'comparison'),
            '--worker',str(resolve(a.worker))]+(['--no-build'] if a.no_build else [])))
    if a.dry_run:
        for stage,command in commands:print(stage+': '+' '.join(command))
        return 0
    output.mkdir(parents=True)
    (output/'source.egg').write_bytes(source.read_bytes())
    digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
    report=dict(schema='structural-source-pipeline/v2',status='running',source=str(source),source_sha256=digest(source),
        stages=[],artifacts={'source.egg':digest(source)},
        key_policy='schema-derived; preserve original merge and choice',runtime_extension='cell-structural-key-runtime/v1',marker_backend='whole-program measured typed-spine selection; self-contained comments',
        acceptance='SK vs SK+MK on pristine and GPU, plus cross-backend whole-public-graph checks; original choices remain unchanged')
    def save(): (output/'pipeline.json').write_text(json.dumps(report,indent=2)+'\n')
    save()
    for stage,command in commands:
        log=output/(stage+'.log')
        record=dict(stage=stage,command=command,log=log.name);report['stages'].append(record);save()
        print('structural-pipeline: '+stage,flush=True)
        try:
            with log.open('w') as f:
                result=subprocess.run(command,cwd=repo,env=dict(os.environ,RAYON_NUM_THREADS='1'),stdout=f,stderr=subprocess.STDOUT)
            record['returncode']=result.returncode
        except OSError as error:
            record.update(returncode=1,error=str(error))
        if record['returncode']:
            report['status']='failed';report['failed_stage']=stage;save()
            print(f'{stage} failed; artifacts retained in {output}; see {log}',file=sys.stderr)
            return record['returncode']
        if stage in ('structural-keys','markers'):
            src,name=(output/'sk/source_sk.egg','source_sk.egg') if stage=='structural-keys' else (output/'mk/source_sk_mk.egg','source_sk_mk.egg')
            (output/name).write_bytes(src.read_bytes());report['artifacts'][name]=digest(src)
        if stage=='structural-keys':report['structural_keys']=json.loads((output/'sk/report.json').read_text())
        if stage=='markers':
            report['marker_summary']=json.loads((output/'mk/report.json').read_text())
        save()
    report['status']='checked' if a.check else 'generated';save()
    print(output/'source_sk.egg');print(output/'source_sk_mk.egg')
    return 0

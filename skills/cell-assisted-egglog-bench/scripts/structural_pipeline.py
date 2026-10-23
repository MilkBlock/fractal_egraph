"""Explicit source -> standalone structural keys -> source-level markers."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def run(a,repo):
    resolve=lambda p:p if p.is_absolute() else repo/p
    source=resolve(a.source);output=resolve(a.output);analysis=resolve(a.analysis)
    if output.exists():raise ValueError('use a fresh pipeline output')
    if not (analysis/'catalog/states').is_dir():raise ValueError('analysis directory must contain catalog/states')
    python=resolve(a.solver_python) if a.solver_python else next((p for p in [repo/'out/cell-spine-venv/bin/python',Path('/Users/mineralsteins/Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python')] if p.is_file()),Path(sys.executable))
    commands=[]
    if not a.no_build:
        commands += [('build',['cargo','build','--offline','--release','--target-dir','target/analyzer','--example','structural_key_source','--example','lhs_ast'])]
    commands += [('structural-keys',[str(python),'-m','tools.cell_spine.structural_keys','--source',str(source),'--output',str(output/'sk')]),
        ('markers',[str(python),'-m','tools.cell_spine.pass3_markers','--source',str(output/'sk/source_sk.egg'),'--analysis',str(analysis),'--output',str(output/'mk'),'--ast-binary',str(repo/'target/analyzer/release/examples/lhs_ast')])]
    if a.check:
        commands.append(('comparison',[sys.executable,str(repo/'skills/cell-assisted-egglog-bench/scripts/bench.py'),'--repo',str(repo),'program-modes',
            '--source',str(output/'source_sk.egg'),'--marked',str(output/'source_sk_mk.egg'),'--output',str(output/'comparison'),
            '--worker',str(resolve(a.worker)),'--native-rulesets','sk-portable,sk-portable-freeze']+(['--no-build'] if a.no_build else [])))
    if a.dry_run:
        for stage,command in commands:print(stage+': '+' '.join(command))
        return 0
    output.mkdir(parents=True)
    (output/'source.egg').write_bytes(source.read_bytes())
    digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
    report=dict(schema='structural-source-pipeline/v1',status='running',source=str(source),source_sha256=digest(source),
        analysis=str(analysis),stages=[],artifacts={'source.egg':digest(source)},
        marker_backend='existing Analyze-backed full-key two-atom source adapter',
        acceptance='SK vs SK+MK on pristine and GPU, plus cross-backend whole-public-graph checks; no claim of original vs SK equivalence',
        explicit_native_rulesets=['sk-portable','sk-portable-freeze'])
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
            src,name=(output/'sk/source_sk.egg','source_sk.egg') if stage=='structural-keys' else (output/'mk/published.egg','source_sk_mk.egg')
            (output/name).write_bytes(src.read_bytes());report['artifacts'][name]=digest(src)
        if stage=='structural-keys':report['structural_keys']=json.loads((output/'sk/report.json').read_text())
        if stage=='markers':
            marker=json.loads((output/'mk/markers.json').read_text())
            report['marker_summary']={k:marker[k] for k in ('analysis_sha256','modified_consumers','selection_scope','variants')}
        save()
    report['status']='checked' if a.check else 'generated';save()
    print(output/'source_sk.egg');print(output/'source_sk_mk.egg')
    return 0

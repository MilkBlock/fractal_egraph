"""Fresh Math capture in an isolated run directory. No fallback to saved data."""
import datetime
import hashlib
import importlib.util
import json
import shutil
import subprocess
import sys
from pathlib import Path

SOURCE = Path('experiments/annotated_export/math_microbenchmark/source.egg')

def destination(root, output):
    if output is None:
        output = Path('out') / ('math-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    return output.resolve() if output.is_absolute() else (root / output).resolve()

def capture_or_reuse(root, driver, output, rounds, fresh, source=None):
    dest = destination(root, output)
    if not fresh:
        marker = dest / 'run.json'
        if not marker.is_file() or json.loads(marker.read_text()).get('status') != 'complete':
            raise ValueError('reuse requires a completed run.json; no fallback to the committed snapshot')
        state=json.loads(marker.read_text());state.update(status='running',phase='reuse-tier2')
        marker.write_text(json.dumps(state,indent=2)+'\n')
        try:
            subprocess.run([sys.executable, str(dest/'experiments/tier2/run.py'), '--driver', str(driver), '--skip-checks'], check=True, cwd=dest)
        except BaseException as error:
            state.update(status='failed',error=str(error));marker.write_text(json.dumps(state,indent=2)+'\n');raise
        state.update(status='complete',phase='complete');marker.write_text(json.dumps(state,indent=2)+'\n')
        print('Reused viewer:', dest/'experiments/tier2/fractal.html', flush=True)
        return
    if dest.exists():raise FileExistsError(dest)
    source_path = (root / (source or SOURCE)).resolve()
    source_bytes = source_path.read_bytes()  # Reject missing input before creating output.
    dest.mkdir(parents=True, exist_ok=False)  # Never replace an earlier run.
    state = {'status':'running', 'requested_rounds':rounds, 'source':str(source_path), 'staged_source':str(SOURCE),
             'source_sha256':hashlib.sha256(source_bytes).hexdigest(),
             'phase':'prepare', 'fallback_used':False}
    marker = dest/'run.json'
    def phase(name):
        state['phase']=name;marker.write_text(json.dumps(state,indent=2)+'\n')
        print(f'[capture] {name}: {dest}',flush=True)
    def run(*cmd):
        subprocess.run([str(x) for x in cmd],cwd=dest,check=True)
    def py(name):
        run(sys.executable,dest/'experiments/tier1_extract'/name)
    try:
        # Copy code and source, never the old native snapshots or analysis JSON.
        for folder in ['experiments/tier1_extract','experiments/tier2']:
            (dest/folder).mkdir(parents=True)
            for p in (root/folder).iterdir():
                if p.suffix=='.py' or p.name.endswith('_template.html'):
                    shutil.copy2(p,dest/folder/p.name)
        for relative in ['rules','experiments/tier1_effects']:
            (dest/relative).mkdir(parents=True,exist_ok=True)
            for p in (root/relative).glob('*.egg'):shutil.copy2(p,dest/relative/p.name)
        for name in ['ir.egg','higher_ir.egg','reduce_ir.egg','reduce.egg']:
            shutil.copy2(root/'experiments/tier2'/name,dest/'experiments/tier2'/name)
        (dest/SOURCE).parent.mkdir(parents=True,exist_ok=True)
        (dest/SOURCE).write_bytes(source_bytes)
        (dest/'experiments/comb_order').mkdir(parents=True)
        phase('build-capture-tools')
        profile='release' if driver.parent.name=='release' else 'debug'
        target=root/'research/target'
        command=['cargo','build','--manifest-path',str(root/'research/Cargo.toml'),'--target-dir',str(target),
                 '--bin','combine_profile','--bin','tier1_export','--bin','tier1_interface_snapshot']
        if profile=='release':command.append('--release')
        subprocess.run(command,cwd=root,check=True)
        binary=target/profile
        phase('trace-tier0')
        run(binary/'combine_profile',SOURCE,'profile.json',rounds)
        raw=json.loads((dest/'profile.json').read_text())
        if raw.get('executed_rounds')!=rounds:
            raise ValueError(f"requested {rounds} rounds, trace executed {raw.get('executed_rounds')}")
        state['executed_rounds']=rounds;state['match_events']=len(raw['events'])
        declarations = [d for d in raw.get('datatypes',[]) if d['name']=='Math']
        if len(declarations)!=1:
            raise ValueError('current importer requires one explicit Math datatype; arbitrary .egg schemas/includes are not supported')
        datatype = declarations[0]['definition']
        (dest/SOURCE.parent/'combined.egg').write_text(datatype+'\n')
        phase('import-tier1')
        spec=importlib.util.spec_from_file_location('math_bridge',root/'research/math_bridge.py')
        bridge=importlib.util.module_from_spec(spec);spec.loader.exec_module(bridge)
        program,manifest=bridge.build(raw)
        state['import_audit']=manifest['audit']
        (dest/'imported.egg').write_text(program)
        (dest/'manifest.json').write_text(json.dumps(manifest)+'\n')
        run(binary/'tier1_export','imported.egg','native.json','--compact')
        native=json.loads((dest/'native.json').read_text());t1=dest/'experiments/tier1_extract'
        saved={'native_egraph':native['native_egraph'],'templates':native['templates'],
               'instances':[{'template':i['template']} for i in native['instances']],
               'source_scope':native['graph_scope'],'capture':{'rounds':rounds,'source':str(source_path),'source_sha256':state['source_sha256']}}
        (t1/'native_templates.json').write_text(json.dumps(saved)+'\n')
        (t1/'tier0_rule_dictionary.json').write_text(json.dumps(raw['rule_labels'])+'\n')
        del saved,native
        phase('extract-tier1')
        run(driver,'schema',SOURCE,'experiments/tier1_extract/source_schema.json')
        py('extract.py')
        run(sys.executable,t1/'prepare_interfaces.py','manifest.json','native.json','profile.json')
        py('interface_program.py')
        run(binary/'tier1_interface_snapshot')
        # Bootstrap the source dictionary for lowering; no previous results are copied.
        used={d['tier0_rule_id'] for d in json.loads((t1/'extraction.json').read_text())['definitions'] if 'tier0_rule_id' in d}
        (t1/'tier0_rules.egg').write_text(datatype+'\n'+'\n'.join(raw['rule_labels'][r]['definition'].replace('\\n','\n') for r in sorted(used))+'\n')
        del raw,manifest,program
        py('lower.py');py('render.py')
        phase('analyze-tier2')
        run(sys.executable,dest/'experiments/tier2/run.py','--driver',driver,'--skip-checks')
        state['status']='complete';state['phase']='complete'
        state['viewer']='experiments/tier2/fractal.html'
        marker.write_text(json.dumps(state,indent=2)+'\n')
        print('Fresh viewer:',dest/state['viewer'],flush=True)
    except BaseException as error:
        state['status']='failed';state['error']=str(error)
        marker.write_text(json.dumps(state,indent=2)+'\n')
        raise

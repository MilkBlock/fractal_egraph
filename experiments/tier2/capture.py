"""Fresh Math capture in an isolated run directory. No fallback to saved data."""
import datetime
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

SOURCE = Path('experiments/annotated_export/math_microbenchmark/source.egg')

def destination(root, output):
    if output is None:
        output = Path('out') / ('math-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    return output.resolve() if output.is_absolute() else (root / output).resolve()

def write_json(path, value):
    with path.open('w') as out:
        json.dump(value,out,separators=(',',':'))
        out.write('\n')

def profile_pipe(command, cwd):
    """Record-framed transport: no whole serialized trace string or trace file."""
    with subprocess.Popen([str(x) for x in command],cwd=cwd,stdout=subprocess.PIPE,text=True) as proc:
        result={};ended=False
        try:
            for line in proc.stdout:
                record=json.loads(line);kind=record[0]
                if ended:raise ValueError('records after end marker')
                if kind=='array':result[record[1]]=[]
                elif kind=='item':result[record[1]].append(record[2])
                elif kind=='value':result[record[1]]=record[2]
                elif kind=='end':ended=True
                else:raise ValueError('unknown capture record')
            if proc.wait()!=0:raise subprocess.CalledProcessError(proc.returncode,command)
            if not ended:raise ValueError('incomplete capture stream')
            return result
        except BaseException:
            if proc.poll() is None:proc.kill()
            proc.wait();raise

def command_pipe(command, lines, cwd, env=None):
    """Stream complete .egg commands into the native importer; decode its result."""
    with subprocess.Popen([str(x) for x in command],cwd=cwd,stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=env) as proc:
        try:
            for line in lines:proc.stdin.write(line+'\n')
            proc.stdin.close()
            result=json.load(proc.stdout)
            if proc.wait()!=0:raise subprocess.CalledProcessError(proc.returncode,command)
            return result
        except BaseException:
            if proc.poll() is None:proc.kill()
            proc.wait();raise

def capture_or_reuse(root, driver, output, rounds, fresh, source=None):
    dest = destination(root, output)
    if not fresh:
        marker = dest / 'run.json'
        if not marker.is_file() or json.loads(marker.read_text()).get('status') != 'complete':
            raise ValueError('reuse requires a completed run.json; no fallback to the committed snapshot')
        state=json.loads(marker.read_text());state.update(status='running',phase='reuse-tier2')
        marker.write_text(json.dumps(state,indent=2)+'\n')
        try:
            subprocess.run([sys.executable, str(dest/'experiments/tier2/run.py'), '--driver', str(driver), '--skip-checks'] + (['--focused'] if state.get('materialization')=='fractal-only' else []), check=True, cwd=dest)
        except BaseException as error:
            state.update(status='failed',error=str(error));marker.write_text(json.dumps(state,indent=2)+'\n');raise
        state.update(status='complete',phase='complete');marker.write_text(json.dumps(state,indent=2)+'\n')
        print('Reused viewer:', dest/'experiments/tier2/fractal.html', flush=True)
        return
    if dest.exists():raise FileExistsError(dest)
    source_path = (root / (source or SOURCE)).resolve()
    source_bytes = source_path.read_bytes()  # Reject missing input before creating output.
    dest.mkdir(parents=True, exist_ok=False)  # Never replace an earlier run.
    state = {'status':'running', 'requested_rounds':rounds, 'schedule_mode':'source' if rounds is None else 'override', 'source':str(source_path), 'staged_source':str(SOURCE),
             'source_sha256':hashlib.sha256(source_bytes).hexdigest(),
             'phase':'prepare', 'fallback_used':False, 'transport':'pipe', 'materialization':'fractal-only'}
    marker = dest/'run.json'
    phase_started=time.perf_counter()
    state['phase_seconds']={}
    def phase(name):
        nonlocal phase_started
        now=time.perf_counter();state['phase_seconds'][state['phase']]=round(now-phase_started,6);phase_started=now
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
        command=[binary/'combine_profile',SOURCE,'-','--bridge-stream']
        if rounds is not None:command.append(rounds)
        raw=profile_pipe(command,dest)
        if rounds is not None and raw.get('executed_rounds')!=rounds:
            raise ValueError(f"requested {rounds} rounds, trace executed {raw.get('executed_rounds')}")
        state['executed_rounds']=raw.get('executed_rounds');state['match_events']=len(raw['events'])
        declarations = [d for d in raw.get('datatypes',[]) if d['name']=='Math']
        if len(declarations)!=1:
            raise ValueError('current importer requires one explicit Math datatype; arbitrary .egg schemas/includes are not supported')
        datatype = declarations[0]['definition']
        (dest/SOURCE.parent/'combined.egg').write_text(datatype+'\n')
        phase('import-tier1')
        spec=importlib.util.spec_from_file_location('math_bridge',root/'research/math_bridge.py')
        bridge=importlib.util.module_from_spec(spec);spec.loader.exec_module(bridge)
        program,manifest=bridge.build(raw,streaming=True)
        state['import_audit']=manifest['audit']
        rule_labels=raw['rule_labels']
        layouts=[{'event':r['id'],'parents':r['parents'],'inputs':r['input_layout'],'outputs':r['output_layout']} for r in manifest['records']]
        del raw,manifest
        import_env=dict(os.environ)
        import_env.setdefault('RAYON_NUM_THREADS','1')
        state['import_threads']=import_env['RAYON_NUM_THREADS']
        native=command_pipe([binary/'tier1_export','-','-','--compact'],program,dest,env=import_env)
        del program
        state['tier1_relation_sizes']=native.get('relation_sizes')
        t1=dest/'experiments/tier1_extract'
        instances={i['event']:i['template'] for i in native['instances']}
        for record in layouts:record['template']=instances[record['event']]
        del instances
        saved={'native_egraph':native['native_egraph'],'templates':native['templates'],
               'instances':[{'template':i['template']} for i in native['instances']],
               'source_scope':native['graph_scope'],'capture':{'rounds':state['executed_rounds'],'source':str(source_path),'source_sha256':state['source_sha256']}}
        write_json(t1/'native_templates.json',saved)
        write_json(t1/'tier0_rule_dictionary.json',rule_labels)
        del saved,native
        phase('extract-tier1')
        run(driver,'schema',SOURCE,'experiments/tier1_extract/source_schema.json')
        py('extract.py')
        interfaces={'native_sha256':hashlib.sha256((t1/'native_templates.json').read_bytes()).hexdigest(),'occurrences':layouts}
        spec=importlib.util.spec_from_file_location('interface_program',t1/'interface_program.py')
        module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        definitions=json.loads((t1/'extraction.json').read_text())
        schema=json.loads((t1/'source_schema.json').read_text())
        interface_lines=module.commands(definitions,interfaces,schema)
        native_interfaces=command_pipe([binary/'tier1_interface_snapshot','-','-'],interface_lines,dest,env=import_env)
        write_json(t1/'native_interfaces.json',native_interfaces)
        del native_interfaces,interfaces,layouts,definitions,schema,interface_lines
        # Bootstrap the source dictionary for lowering; no previous results are copied.
        used={d['tier0_rule_id'] for d in json.loads((t1/'extraction.json').read_text())['definitions'] if 'tier0_rule_id' in d}
        (t1/'tier0_rules.egg').write_text(datatype+'\n'+'\n'.join(rule_labels[r]['definition'].replace('\\n','\n') for r in sorted(used))+'\n')
        del rule_labels
        phase('analyze-tier2')
        run(sys.executable,dest/'experiments/tier2/run.py','--driver',driver,'--skip-checks','--focused')
        phase('complete')
        state['status']='complete';state['phase']='complete'
        state['viewer']='experiments/tier2/fractal.html'
        marker.write_text(json.dumps(state,indent=2)+'\n')
        print('Fresh viewer:',dest/state['viewer'],flush=True)
    except BaseException as error:
        state['status']='failed';state['error']=str(error)
        try:marker.write_text(json.dumps(state,indent=2)+'\n')
        except OSError as status_error:print(f'Could not save failure status: {status_error}',file=sys.stderr)
        raise

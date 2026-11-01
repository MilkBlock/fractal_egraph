"""Run a loaded analyzer library on Metal; no native engine during execution."""
import argparse, hashlib, json, subprocess, time
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from eggfuse.compiler.targets.metal.generic_cell import groups, lower
from eggfuse.compiler.targets.metal.compiler import encode_initial

HERE = Path(__file__).resolve().parent

def build_worker(output):
    output.mkdir(parents=True, exist_ok=True)
    source = ROOT / 'engine/metal/generic_cell.swift'; worker = output / 'generic-cell-worker'
    from eggfuse.engine.scheduled import worker_source
    text=worker_source();fingerprint = hashlib.sha256(text.encode()).hexdigest()
    stamp = output / 'generic-cell-worker.sha256'
    if not worker.exists() or not stamp.exists() or stamp.read_text() != fingerprint:
        materialized=output/'generic-cell-worker.swift';materialized.write_text(text)
        subprocess.run(['xcrun','swiftc','-O',str(materialized),'-o',str(worker)],check=True)
        stamp.write_text(fingerprint)
    return worker.resolve()

def run_library(library, build, output, capacity=256, match_capacity=1024, quantum=8, grow=True,marker_program=None,compiled_program=None,region_program=None,audit_numeric=False):
    from eggfuse.ir.library import adapt,decode
    recipe_library=library if library.get('schema')=='eggfuse-recipe-library/v1' else adapt(library)
    library=decode(recipe_library)
    if quantum <= 0: raise ValueError('quantum must be positive; it is dispatch batching, not a termination bound')
    output.mkdir(parents=True, exist_ok=False)
    (output/'recipe-library.json').write_text(json.dumps(recipe_library))
    worker = build_worker(build); reports=[]; rejected=list(library.get('rejected',[]))
    provenance=dict(binary_sha256=hashlib.sha256(worker.read_bytes()).hexdigest(),
        source_sha256=(build/'generic-cell-worker.sha256').read_text(),
        source_components={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [ROOT/'engine/metal/generic_cell.swift',ROOT/'engine/metal/scheduled_matrix.swift',ROOT/'eggfuse/engine/scheduled.py']},
        source='engine/metal/generic_cell.swift')
    for pi, instances, cores in groups(library):
        cap=max(capacity,max(len(c['initial']) for c in cores));mc=match_capacity
        folder=output/f'program-{pi}';folder.mkdir();todo=[];attempts=[];finished=[]
        for i,c in zip(instances,cores):
            if i.get('observed_completion','Saturated')!='Saturated':
                rejected.append(dict(unit=i['unit'],reason='unfinished observed Cell; no finite completion permission'))
            else:todo.append((i,c))
        while todo:
            try:
                compilation={**todo[0][1],'cell_input_constants':[c['initial'] for _,c in todo]}
                if compiled_program is None:p=lower(compilation,cap,mc,marker_program=marker_program,region_program=region_program)
                else:
                    folder_art=compiled_program.parent;manifest=json.loads((folder_art/'manifest.json').read_text())
                    if manifest.get('backend') not in ('local','regions'):raise ValueError('compiled artifact is not a local/region program')
                    for filename,expected in manifest['files'].items():
                        if Path(filename).name!=filename or hashlib.sha256((folder_art/filename).read_bytes()).hexdigest()!=expected:raise ValueError('compiled local artifact checksum mismatch')
                    artifact_core=json.loads((folder_art/'core.json').read_text())
                    if artifact_core!=todo[0][1]:raise ValueError('compiled local input differs')
                    if marker_program is None or manifest['marker_sha256']!=marker_program['sha256']:raise ValueError('compiled local marker contract differs')
                    from eggfuse.compiler.marker_ir import validate
                    validate(marker_program,artifact_core)
                    p=json.loads(compiled_program.read_text());grow=False
                    if p['shader']!=(folder_art/'kernel.metal').read_text() or p['tables']!=artifact_core['tables']:raise ValueError('compiled local kernel/schema differs')
                    from eggfuse.engine.artifacts import validate_recipe_artifact
                    validate_recipe_artifact(folder_art,manifest,p,artifact_core)
                frames=[]
                for instance,core in todo:
                    anchors,arena,flat=encode_initial(core,p['stride']-4,{s:i for i,s in enumerate(p['strings'])},native_order=True,key_layout=p.get("structural_key"))
                    if len(core['initial'])>p['capacity'] or len(anchors)>=p['id_capacity'] or len(arena)>p['set_words']:
                        raise ValueError('initial instance exceeds shared arena; increase capacity')
                    frames.append(dict(initial_rows=flat,initial_sets=arena,anchors=anchors,
                        meta=[len(core['initial']),len(anchors)+1,1,0,0,0,0,0,len(core['initial'])+1]))
                p.update(instances=frames,quantum=quantum,batch_audit_numeric=audit_numeric)
            except (ValueError,KeyError) as e:
                rejected.extend(dict(unit=i['unit'],reason=str(e)) for i,_ in todo);break
            attempt=folder/f'attempt-{len(attempts)}';attempt.mkdir()
            (attempt/'program.json').write_text(json.dumps(p));(attempt/'kernel.metal').write_text(p['shader'])
            started=time.perf_counter()
            with (attempt/'worker.log').open('w') as log:
                subprocess.run([str(worker),str((attempt/'program.json').resolve()),str((attempt/'result.json').resolve())],stdout=log,stderr=subprocess.STDOUT,check=True)
            report=json.loads((attempt/'result.json').read_text());report['wall_seconds_including_compile_export']=time.perf_counter()-started
            report['executed_instances']=len(todo)
            attempts.append({k:v for k,v in report.items() if k!='instances'})
            retry=[];errors=[]
            for (i,core),f,result in zip(todo,frames,report['instances']):
                result['unit']=i['unit'];error=result['meta'][4]
                if grow and error in (1,2,11):retry.append((i,core));errors.append(error);continue
                unit=folder/f'cell-{i["unit"]}';unit.mkdir()
                view={**p,**f};view.pop('instances');view.pop('shader')
                (unit/'program.json').write_text(json.dumps(view));(unit/'core.json').write_text(json.dumps(core))
                (unit/'gpu-state.json').write_text(json.dumps(result));finished.append(result)
            # Failed private attempts published nothing. Completed instances are
            # retained and are never replayed because a neighboring cell grew.
            if 1 in errors:cap*=2
            if 2 in errors or 11 in errors:mc*=2
            todo=retry
        reports.append(dict(program=pi,instances=finished,attempts=attempts,
                            inference_seconds=sum(r['inference_seconds'] for r in attempts),
                            gpu_seconds=sum(r['gpu_seconds'] for r in attempts)))
    result={'schema':'generic-cell-results/v1','programs':reports,'rejected':rejected,
            'complete':not rejected and all(s['completed'] for r in reports for s in r['instances']),
            'instances':sum(len(r['instances']) for r in reports),
            'contract':'full local exact rules; no rule names or observed endpoints used for execution'}
    result['worker_provenance']=provenance
    (output/'results.json').write_text(json.dumps(result,indent=2)+'\n');return result

def source_library(library):
    if library.get('schema')=='eggfuse-recipe-library/v1':
        from eggfuse.ir.library import decode
        library=decode(library)
    source=library.get('source_program')
    if not source or source.get('status')!='ready':raise ValueError('analyzer has no supported original program entry: '+str(source))
    core=dict(source['core']);initial=core.pop('initial')
    return dict(schema='analyzer-device-cells/v1',programs=[core],instances=[dict(unit=0,program=0,initial=initial)],
                scope='original global input and schedule, one exact context; not decomposed into independent Cells')

def run_source(source,library_path,build,output,check=True,capacity=256,match_capacity=1024):
    library=json.loads(library_path.read_text());fingerprint=library['source_schedule']['source_fingerprint']
    data=source.read_bytes();h=0xcbf29ce484222325
    for b in data:h=((h^b)*0x100000001b3)&((1<<64)-1)
    if fingerprint!={'bytes':len(data),'fnv1a64':f'{h:016x}'}:raise ValueError('analyzer/source fingerprint mismatch')
    result=run_library(source_library(library),build,output,capacity,match_capacity)
    if not result['complete']:raise ValueError('generic device execution incomplete; inspect private diagnostics')
    folder=output/'program-0/cell-0'
    result['original_source_sha256']=hashlib.sha256(data).hexdigest()
    result['scope']='original source schedule in one GPU-owned context; no decomposition/acceleration claim'
    if check:
        from verify_native_source import verify
        result['native_verification']=verify(source,build,folder,output/'native')
    else:result['native_verification']=None
    (output/'report.json').write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--library',type=Path,required=True);p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--capacity',type=int,default=256);p.add_argument('--match-capacity',type=int,default=1024);p.add_argument('--quantum',type=int,default=8);p.add_argument('--no-grow',action='store_true');p.add_argument('--source-program',action='store_true');a=p.parse_args()
    library=json.loads(a.library.read_text());library=source_library(library) if a.source_program else library
    r=run_library(library,a.build,a.output,a.capacity,a.match_capacity,a.quantum,not a.no_grow)
    print(json.dumps({k:v for k,v in r.items() if k!='programs'},indent=2))
    if not r['complete']:raise SystemExit(1)

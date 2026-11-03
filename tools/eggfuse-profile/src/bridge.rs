//! Temporary wire adapter for the existing Python artifact API.
//!
//! Its recomputed certificate contains tuples, while the serialized certificate
//! contains JSON arrays. Normalize only that description to its JSON wire form;
//! retain the existing source, manifest, recipe and full certificate checks.
//! No Python file or GPU execution function is replaced.
pub const RUN: &str = r#"
import json,sys
from pathlib import Path
from eggfuse.compiler.targets.metal import resident_schedules
original_description=resident_schedules.description
def wire_description(*args,**kwargs):
    info,code,empty=original_description(*args,**kwargs)
    return json.loads(json.dumps(info)),code,empty
resident_schedules.description=wire_description
from eggfuse.engine.metal import run_artifact
r=run_artifact(Path(sys.argv[1]),Path(sys.argv[2]),Path(sys.argv[3]),check=False)
assert r['complete_schedule']
print(json.dumps({'complete_schedule':True,'inference_seconds':r['inference_seconds'],'certificate_adapter':'tuple/list JSON normalization only'}))
"#;

/// The same frontend -> from_core -> resident lower used by the retained
/// 162.65-second restoration benchmark. Serialize the existing APIs' output;
/// do not create another inference implementation or silently disable Cell.
pub const COMPILE_RESIDENT: &str = r#"
import json,sys,subprocess,hashlib,shutil
from pathlib import Path
from eggfuse.analyzer.regions import from_core
from eggfuse.ir.recipe import build
from eggfuse.compiler.marker_ir import build as marker_contract
from eggfuse.compiler.targets.metal.generic_cell import lower
source,frontend,folder=map(Path,sys.argv[1:4]);folder.mkdir(parents=True)
text=source.read_bytes()
core=json.loads(subprocess.run([str(frontend),str(source)],input='',text=True,stdout=subprocess.PIPE,check=True).stdout.splitlines()[0])
recipe=build(core);region=from_core(core)
program=lower(core,int(sys.argv[4]),int(sys.argv[5]),region_program=region)
assert program['marker_program'] is None
marker=marker_contract({'markers':[],'cost_model':'disabled in resident control','selection_scope':'no source transformation'},core,text,b'')
program['recipe_ir']={'schema':recipe['schema'],'sha256':recipe['sha256'],'scope':recipe['scope']}
for name,value in [('core.json',core),('program.json',program),('region-program.json',region),('recipe-program.json',recipe),('marker-program.json',marker)]:
    (folder/name).write_text(json.dumps(value))
(folder/'kernel.metal').write_text(program['shader']);(folder/'source_sk_mk.egg').write_bytes(text)
files={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in folder.iterdir()}
manifest={'schema':'eggfuse-device-artifact/v1','backend':'regions','profile_compilation':'resident-core','execution':dict(stage_contexts=False,joint_polyspine=False,block_merge=False,matrix_saturate=False),'source_sha256':hashlib.sha256(text).hexdigest(),'marker_sha256':marker['sha256'],'files':files}
(folder/'manifest.json').write_text(json.dumps(manifest))
print(json.dumps({'backend':'regions','source_markers':False,'multi_stage_cell':True,'tables':len(core['tables']),'initial_rows':len(core['initial'])}))
"#;

"""One serial replay per backend and pass; output is an append-only JSONL file."""
from pathlib import Path
import os,sys,json,tempfile,hashlib,subprocess,time,resource
cap,compiler,bake,donor,linker,native,cell,scratch,output=map(Path,sys.argv[1:])
def sha(p):
    with p.open("rb") as f:return hashlib.file_digest(f,"sha256").hexdigest()
env={k:v for k,v in os.environ.items() if not k.startswith(("CELL_CPU_","EGG_LAYOUT_"))}
env.update(RAYON_NUM_THREADS="1",CELL_CPU_STAGE_SPINE="1",CELL_CPU_STAGE_SPINE_RECEIPTS="1",CELL_CPU_SHARED_NODE_SCHEDULE="1",CELL_CPU_JOIN_INTERSECTION="1",CELL_CPU_SIEVE="1",CELL_CPU_FREE_JOIN="planned",CELL_CPU_KNOWN_RESULTS="static",CELL_CPU_RANGE_GUARDS="1",CELL_CPU_FD_KEYS="0",CELL_CPU_SCALAR_MERGE="0",CELL_CPU_SLOT_TRANSFER="off")
cases=json.loads((cap/"manifest.json").read_text())["cases"]
done={(x["case"],x["pass"]) for x in map(json.loads,output.read_text().splitlines())} if output.exists() else set()
def run(args):
    v=subprocess.run(list(map(str,args)),capture_output=True,env=env,timeout=300)
    if v.returncode:raise ValueError(v.stderr.decode(errors="replace")[-550:])
    return v.stdout.decode()
for pidx in (3,1,2):
 for index,c in enumerate(cases):
    name=c["source"].split("/benchmarks/passing/")[1]
    if (name,pidx) in done:continue
    case=cap/Path(c["directory"]).name
    p=c["passes"][pidx-1];src=case/Path(p["input"]).name
    assert sha(src)==p["input_sha256"]
    orig=next((case/"capture").glob(f"*/pass-{pidx}.json"))
    prod=json.loads(orig.read_text());sched=prod["schedule_profile"]
    row=dict(case=name,pass_number=pidx,source_sha256=c["source_sha256"],flat_sha256=sha(src),raw_sha256=sha(orig.with_suffix(".egg")),prod_sched_ms=1000*(sched["saturate_seconds"]+sched["run_seconds"]),prod_exec_ms=1000*prod["execution_seconds"],prod_extract_ms=1000*prod["extraction_wall_seconds"],prod_nodes=prod["nodes"],prod_classes=prod["eclasses"])
    row["pass"]=pidx
    with tempfile.TemporaryDirectory(dir=scratch) as work:
        d=Path(work)
        def time_native():row["native_ms"]=float(run([native,src]).strip().splitlines()[-1].split(' ms')[0].split()[-1])
        def time_cell():
            start=time.monotonic()
            run([compiler,"compile","--source",src,"--bake-source",bake,"--emit-free-join-entry","--output",d/"compiled"])
            row["units"]=int(run([linker,donor,d/"compiled/program.json",d/"ready.json"]).strip())
            row["prepare_s"]=round(time.monotonic()-start,3)
            run([cell,"--program",d/"ready.json","--output",d/"out"])
            j=json.loads((d/"out/report.json").read_text())
            assert j["status"]=="complete"
            row.update(cell_ms=1000*j["inference_seconds"],runs=len(j["steps"]),bindings=j["stats"]["bindings"],resumed=j["stage_spine"]["stats"]["resumed_steps"],state_sha256=sha(d/"out/state.json"))
        for f in ((time_native,time_cell) if index%2==0 else (time_cell,time_native)):
            try:f()
            except Exception as e:row[f.__name__+"_error"]=str(e)
    with output.open('a') as f:print(json.dumps(row),file=f)
    print(index,pidx,name,row.get("native_ms"),row.get("cell_ms"),row.get("time_cell_error","")[:80],flush=True)

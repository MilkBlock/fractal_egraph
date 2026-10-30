use crate::{
    Options,
    cache::{
        self, Cache, Entry, files_under, fingerprints, hash_bytes, hash_file, read_json, write_json,
    },
    suite::Case,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};

fn executable(path: &Path) -> Result<PathBuf> {
    if path.components().count() > 1 {
        return Ok(std::path::absolute(path)?);
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let p = dir.join(path);
        if p.is_file() {
            return Ok(p);
        }
    }
    anyhow::bail!("executable not found: {}", path.display())
}
fn text_command(program: &Path, args: &[&str]) -> Result<String> {
    let p = Command::new(program).args(args).output()?;
    ensure!(
        p.status.success(),
        "{} failed: {}",
        program.display(),
        String::from_utf8_lossy(&p.stderr)
    );
    Ok(String::from_utf8(p.stdout)?.trim().into())
}
fn invoke(o: &Options, args: Vec<String>, log: &Path) -> Result<()> {
    println!("       log: {}", log.display());
    write_json(&log.with_extension("command.json"), &json!(args))?;
    let f = fs::File::create(log)?;
    let started = Instant::now();
    let status = Command::new(&args[0])
        .args(&args[1..])
        .current_dir(&o.root)
        .env("EGGFUSE_METAL_CACHE", o.cache.join("metal-cache"))
        // The current debug analyzer's worker threads overflow the default stack
        // on genuine Pass 3. This changes stack capacity, not RipSat budgets.
        .env(
            "RUST_MIN_STACK",
            std::env::var("RUST_MIN_STACK").unwrap_or("67108864".into()),
        )
        .stdout(Stdio::from(f.try_clone()?))
        .stderr(Stdio::from(f))
        .status()?;
    ensure!(
        status.success(),
        "command exited {status}; see {}",
        log.display()
    );
    println!("       finished in {:.2}s", started.elapsed().as_secs_f64());
    Ok(())
}
fn strings(args: &[&Path]) -> Vec<String> {
    args.iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}
fn python_script(python: &Path, script: &Path, args: Vec<String>) -> Vec<String> {
    let mut c = strings(&[python, script]);
    c.extend(args);
    c
}
fn copy_tree(source: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for p in files_under(source, &[])? {
        let to = dest.join(p.strip_prefix(source)?);
        fs::create_dir_all(to.parent().unwrap())?;
        fs::copy(p, to)?;
    }
    Ok(())
}
fn snapshot_worker(build: &Path, dest: &Path, metal_cache: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for name in [
        "generic-cell-worker",
        "generic-cell-worker.swift",
        "generic-cell-worker.sha256",
    ] {
        fs::copy(build.join(name), dest.join(name))?;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(metal_cache, dest.join("metal-cache"))?;
    Ok(())
}
fn stage_note(e: &Entry) -> Value {
    json!({"cache_hit":e.hit,"key":e.key,"created_unix":e.created,"directory":e.dir})
}
fn label_dashboard(folder: &Path, case: &Case) -> Result<()> {
    let path = folder.join("index.html");
    let text = fs::read_to_string(&path)?;
    // Only the generated presentation changes; existing Python stays intact.
    fs::write(
        path,
        text.replace(
            "真实 Raytrace Pass 3",
            &format!("真实 {} Pass 3", case.name),
        ),
    )?;
    Ok(())
}

pub fn run(o: &Options, cases: &[Case]) -> Result<()> {
    let started = Instant::now();
    fs::create_dir_all(&o.cache)?;
    let cache_root = o.cache.canonicalize()?;
    let mut lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(cache_root.join("suite.lock"))?;
    lock.try_lock()
        .context("another profiler owns this cache; do not overlap GPU measurements")?;
    lock.set_len(0)?;
    writeln!(lock, "{}", std::process::id())?;
    let cache = Cache {
        root: cache_root.join("cache"),
        only: o.only,
        plan: o.plan,
    };
    let python = executable(&o.python)?;
    let solver = if o.pipeline {
        executable(&o.solver)?
    } else {
        python.clone()
    };
    let tools=o.tools.canonicalize().context("native helpers missing; build egg_layout, lhs_ast, structural_key_source and eggfuse_frontend")?;
    let native = if o.pipeline {
        vec![
            tools.join("lhs_ast"),
            tools.join("structural_key_source"),
            tools.join("eggfuse_frontend"),
            tools.parent().unwrap().join("egg_layout"),
        ]
    } else {
        vec![tools.join("eggfuse_frontend")]
    };
    let native_hash = fingerprints(native)?;
    let platform = json!({"os":text_command(Path::new("sw_vers"),&["-productVersion"])? ,"xcode":text_command(Path::new("xcodebuild"),&["-version"])?});
    let displays: Value = serde_json::from_str(&text_command(
        Path::new("system_profiler"),
        &["SPDisplaysDataType", "-json"],
    )?)?;
    let gpu:Vec<_>=displays["SPDisplaysDataType"].as_array().context("GPU inventory")?.iter().map(|g|json!({"model":g["sppci_model"],"cores":g["sppci_cores"],"metal":g["spdisplays_metal"]})).collect();
    let mut env: BTreeMap<_, _> = std::env::vars()
        .filter(|(k, _)| {
            k.starts_with("EGGFUSE_")
                || k.starts_with("EGG_LAYOUT_")
                || k.starts_with("RIPEN_")
                || k == "RUST_MIN_STACK"
                || k == "DEVELOPER_DIR"
        })
        .collect();
    env.entry("RUST_MIN_STACK".into())
        .or_insert("67108864".into());
    let solver_info = if o.pipeline {
        Some(text_command(&solver,&["-c","import sys,importlib.metadata as m; print((sys.version,sys.prefix,m.version('pyscipopt'),m.version('numpy')))"])
        .context("set EGGFUSE_SOLVER_PYTHON or --solver-python to the existing marker-solver environment")?)
    } else {
        None
    };
    let machine = json!({"platform":platform,"gpu":gpu,"env":env,"python":text_command(&python,&["--version"])?});
    let mut compiler_files = Vec::new();
    for dir in [
        "eggfuse/analyzer",
        "eggfuse/compiler",
        "eggfuse/ir",
        "tools/cell_spine",
    ] {
        compiler_files.extend(files_under(&o.root.join(dir), &["py"])?);
    }
    compiler_files.extend(files_under(&o.root.join("engine/metal"), &["metal"])?);
    let compiler_code = fingerprints(compiler_files)?;
    let mut runtime_files = files_under(&o.root.join("eggfuse/engine"), &["py"])?;
    runtime_files.extend(files_under(&o.root.join("engine/metal"), &["swift"])?);
    runtime_files.extend([
        o.root
            .join("experiments/device_backend/run_generic_cells.py"),
        o.root
            .join("experiments/device_backend/verify_native_source.py"),
    ]);
    let runtime_code = fingerprints(runtime_files)?;
    let scripts = o.root.join("skills/eggfuse-metal-profile/scripts");
    let profiler_code = fingerprints(files_under(&scripts, &["py", "swift"])?)?;
    let adapter = if let Some(c) = &o.compiler {
        let p = executable(c)?;
        json!({"program":p,"sha256":hash_file(&p)?})
    } else {
        json!({"program":python,"version":machine["python"]})
    };
    let build = cache_root.join("runtime");
    fs::create_dir_all(&build)?;
    let frontend = tools.join("eggfuse_frontend");
    if hash_file(&build.join("frontend")).ok() != Some(hash_file(&frontend)?) {
        fs::copy(frontend, build.join("frontend"))?;
    }
    fs::create_dir_all(cache_root.join("metal-cache"))?;
    let suite = read_json(&o.suite)?;
    let bake = if o.pipeline && suite.get("bake").is_some() {
        let bytes = crate::suite::input(o.suite.parent().unwrap(), &suite["bake"])?;
        let path = cache_root.join("sources/bake.egg");
        fs::create_dir_all(path.parent().unwrap())?;
        if fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
            fs::write(&path, bytes)?;
        }
        Some(path)
    } else {
        None
    };
    let mut results = Vec::new();
    let mut failures = Vec::new();
    for case in cases {
        println!("\n{} / complete Pass 3", case.name);
        match run_case(
            o,
            case,
            &cache,
            &python,
            &solver,
            &scripts,
            &build,
            bake.as_deref(),
            json!({"code":compiler_code,"native_tools":native_hash,"solver":solver_info,"adapter":adapter,"machine":platform,"environment":machine["env"]}),
            json!({"code":runtime_code,"machine":machine}),
            profiler_code.clone(),
        ) {
            Ok(r) => results.push(r),
            Err(e) => {
                eprintln!("{}: {e:#}", case.name);
                failures.push(json!({"case":case.name,"error":format!("{e:#}")}));
            }
        }
    }
    let report = json!({"schema":"eggfuse-profile-suite-results/v1","generated_unix":cache::now(),"driver_wall_seconds":started.elapsed().as_secs_f64(),"configuration":if o.pipeline{"source-marker-pipeline"}else{"resident-core"},"measurement":"actual resident GPU Pass 3; inference excludes compiler/setup/export; cached measurements retain original timestamps","bake":if o.pipeline{suite.get("bake")}else{None},"cases":results,"failures":failures,"hardware_requested":o.hardware,"capture_requested":o.capture});
    let report_name = if report["failures"].as_array().unwrap().is_empty() {
        "latest"
    } else {
        "last-failure"
    };
    write_json(&cache_root.join(format!("{report_name}.json")), &report)?;
    let md = markdown(&report);
    fs::write(cache_root.join(format!("{report_name}.md")), &md)?;
    println!(
        "\n{md}\n{}",
        cache_root.join(format!("{report_name}.json")).display()
    );
    ensure!(
        report["failures"].as_array().unwrap().is_empty(),
        "some cases did not complete; retained successful cases and failure logs"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_case(
    o: &Options,
    case: &Case,
    cache: &Cache,
    python: &Path,
    solver: &Path,
    scripts: &Path,
    build: &Path,
    bake: Option<&Path>,
    compiler: Value,
    runtime: Value,
    profiler: Value,
) -> Result<Value> {
    let srcdir = o.cache.join("sources").join(&case.name);
    fs::create_dir_all(&srcdir)?;
    let source = srcdir.join("pass-3.egg");
    if fs::read(&source).ok().as_deref() != Some(case.bytes.as_slice()) {
        fs::write(&source, &case.bytes)?;
    }
    let compile_inputs = json!({"source":hash_bytes(&case.bytes),"mode":if o.pipeline{"pipeline"}else{"resident-core"},"wire_compiler":if o.pipeline{None}else{Some(hash_bytes(crate::bridge::COMPILE_RESIDENT.as_bytes()))},"bake":bake.map(hash_file).transpose()?,"capacity":case.spec["capacity"],"match_capacity":case.spec["match_capacity"],"multi_stage_cell":true,"compiler":compiler});
    let compiled=cache.stage(&format!("{}/compile",case.name),compile_inputs,false,|dir|{
        let pipeline=o.cache.join("compiler/pipeline");
        let artifact=dir.join("artifact");
        if !o.pipeline {
            invoke(o,vec![python.display().to_string(),"-c".into(),crate::bridge::COMPILE_RESIDENT.into(),source.display().to_string(),build.join("frontend").display().to_string(),artifact.display().to_string(),case.spec["capacity"].to_string(),case.spec["match_capacity"].to_string()],&dir.join("compile.log"))?;
        } else {
        let mut cmd=if let Some(c)=&o.compiler {vec![executable(c)?.display().to_string(),"compile".into()]}else{vec![python.display().to_string(),"-m".into(),"eggfuse".into(),"compile".into()]};
        cmd.extend(vec!["--source".into(),source.display().to_string(),"--output".into(),pipeline.display().to_string(),"--build".into(),build.display().to_string(),"--tools".into(),o.tools.display().to_string(),"--solver-python".into(),solver.display().to_string(),"--multi-stage-cell".into(),"--stop-after".into(),"device".into(),"--capacity".into(),case.spec["capacity"].to_string(),"--match-capacity".into(),case.spec["match_capacity"].to_string()]);
        if let Some(bake)=bake {cmd.extend(["--bake-source".into(),bake.display().to_string()]);}
        invoke(o,cmd,&dir.join("compile.log"))?;
        copy_tree(&pipeline.join("eggfuse_related_files"),&artifact)?;
        fs::copy(pipeline.join("pipeline.json"),dir.join("pipeline.json"))?;
        }
        let m=read_json(&artifact.join("manifest.json"))?;ensure!(m["backend"]=="regions","expected canonical multi-stage resident artifact");
        for (name,h) in m["files"].as_object().context("artifact files")? {ensure!(hash_file(&artifact.join(name))?==h.as_str().context("artifact hash")?,"invalid compiled artifact {name}");}
        let p=read_json(&artifact.join("program.json"))?;
        let detail=json!({"artifact":artifact,"artifact_hash":hash_file(&artifact.join("program.json"))?,"marker_tables":p["marker_program"]["tables"].as_array().map_or(0,|a|a.len()),"backend":"regions","configuration":if o.pipeline{"source-marker-pipeline"}else{"resident-core"}});
        Ok((detail,files_under(dir,&["json","egg","metal"])?))
    })?;
    let artifact = PathBuf::from(
        compiled.detail["artifact"]
            .as_str()
            .context("compiled artifact")?,
    );
    let baseline=cache.stage(&format!("{}/baseline",case.name),json!({"artifact":compiled.detail,"runtime":runtime,"wire_adapter_sha256":hash_bytes(crate::bridge::RUN.as_bytes()),"protocol":1,"clean":true}),o.fresh,|dir|{
        let out=dir.join("run");
        invoke(o,vec![python.display().to_string(),"-c".into(),crate::bridge::RUN.into(),artifact.display().to_string(),build.display().to_string(),out.display().to_string()],&dir.join("run.log"))?;
        let report=read_json(&out.join("report.json"))?;ensure!(report["complete_schedule"]==true,"incomplete source schedule");
        let attempt=out.join("program-0/attempt-0");let r=read_json(&attempt.join("result.json"))?;
        ensure!(r["instances"].as_array().context("result instances")?.iter().all(|i|i["completed"]==true&&i["meta"][4]==0),"engine reported incomplete/error state");
        let frozen=dir.join("worker");snapshot_worker(build,&frozen,&o.cache.join("metal-cache"))?;
        ensure!(r["worker_sha256"].as_str()==Some(hash_file(&frozen.join("generic-cell-worker"))?.as_str()),"worker changed during execution");
        let detail=json!({"attempt":attempt,"build":frozen,"inference_seconds":r["inference_seconds"],"gpu_seconds":r["gpu_seconds"],"setup_seconds":r["shader_compile_seconds"],"pipeline_cache":r["pipeline_cache"],"pipeline_compile_seconds":r["pipeline_compile_seconds"],"private_runs":r["private_schedule_runs"],"rows":r["instances"][0]["meta"][0],"runs":r["instances"][0]["meta"][6],"dispatches":r["dispatches"],"worker_sha256":r["worker_sha256"],"execution":report["region_execution"]});
        let mut outputs=vec![attempt.join("program.json"),attempt.join("result.json"),out.join("report.json")];outputs.extend(files_under(&frozen,&[])?);Ok((detail,outputs))
    })?;
    let attempt = PathBuf::from(baseline.detail["attempt"].as_str().unwrap());
    let frozen = PathBuf::from(baseline.detail["build"].as_str().unwrap());
    let reference = fingerprints([attempt.join("program.json"), attempt.join("result.json")])?;
    let measured = cache.stage(
        &format!("{}/encoders", case.name),
        json!({"reference":reference,"profiler":profiler,"runtime":runtime}),
        o.fresh,
        |dir| {
            let out = dir.join("profile");
            invoke(
                o,
                python_script(
                    python,
                    &scripts.join("resident_full.py"),
                    vec![
                        "--run".into(),
                        attempt.display().to_string(),
                        "--build".into(),
                        frozen.display().to_string(),
                        "--output".into(),
                        out.display().to_string(),
                    ],
                ),
                &dir.join("profile.log"),
            )?;
            let summary = read_json(&out.join("summary.json"))?;
            ensure!(
                summary["verification"]["full_state_and_history_equal"] == true,
                "profile changed state/history"
            );
            let empty = dir.join("no-hardware.json");
            write_json(
                &empty,
                &json!({"verification":summary["verification"],"windows":[]}),
            )?;
            invoke(
                o,
                python_script(
                    python,
                    &scripts.join("dashboard.py"),
                    vec![
                        "--profile".into(),
                        out.join("summary.json").display().to_string(),
                        "--hardware".into(),
                        empty.display().to_string(),
                        "--output".into(),
                        dir.join("dashboard").display().to_string(),
                    ],
                ),
                &dir.join("dashboard.log"),
            )?;
            Ok((
                json!({"profile":out,"dashboard":dir.join("dashboard/index.html")}),
                vec![
                    out.join("summary.json"),
                    out.join("provenance.json"),
                    dir.join("dashboard/index.html"),
                ],
            ))
        },
    )?;
    let profile = PathBuf::from(measured.detail["profile"].as_str().unwrap());
    let summary = read_json(&profile.join("summary.json"))?;
    let view=cache.stage(&format!("{}/view",case.name),json!({"profile":hash_file(&profile.join("summary.json"))?,"renderer":hash_file(&scripts.join("dashboard.py"))?,"case":case.name,"label_version":1}),false,|dir|{
        let empty=dir.join("no-hardware.json");write_json(&empty,&json!({"verification":summary["verification"],"windows":[]}))?;
        let output=dir.join("dashboard");
        invoke(o,python_script(python,&scripts.join("dashboard.py"),vec!["--profile".into(),profile.join("summary.json").display().to_string(),"--hardware".into(),empty.display().to_string(),"--output".into(),output.display().to_string()]),&dir.join("dashboard.log"))?;
        label_dashboard(&output,case)?;
        Ok((json!({"dashboard":output.join("index.html")}),vec![output.join("index.html"),output.join("data.json")]))
    })?;
    let mut extra = json!({});
    if o.hardware || o.capture {
        let template_options: Value = if o.hardware {
            let mut options: Value = serde_json::from_str(&text_command(
                Path::new("xcrun"),
                &[
                    "xctrace",
                    "record",
                    "--template",
                    "Cell Metal Limiters",
                    "--show-recording-options",
                ],
            )?)?;
            ensure!(
                options.get("Time Profiler").is_some(),
                "counter template lacks Time Profiler"
            );
            options["Time Profiler"]["recordWaitingThreads"] = json!(true);
            options
        } else {
            Value::Null
        };
        let windows = select_window(
            &summary,
            baseline.detail["dispatches"]
                .as_u64()
                .context("dispatch count")?,
            if o.hardware { 16 } else { 1 },
        )?;
        for mode in ["counters", "capture"] {
            if (mode == "counters" && !o.hardware) || (mode == "capture" && !o.capture) {
                continue;
            }
            let count = if mode == "capture" { 1 } else { windows.1 };
            let entry=cache.stage(&format!("{}/{mode}",case.name),json!({"reference":reference,"profiler":profiler,"runtime":runtime,"start":windows.0,"count":count,"template":"Cell Metal Limiters","recording_options":if mode=="counters"{template_options.clone()}else{Value::Null}}),o.fresh,|dir|{
                let out=dir.join("window");invoke(o,python_script(python,&scripts.join("resident_window.py"),vec!["--run".into(),attempt.display().to_string(),"--build".into(),frozen.display().to_string(),"--output".into(),out.display().to_string(),"--mode".into(),mode.into(),"--window".into(),format!("{}:{count}",windows.0)]),&dir.join("window.log"))?;
                let replay=read_json(&out.join("replay.json"))?;ensure!(replay["full_state_and_history_equal"]==true,"window changed state/history");
                let mut outputs=vec![out.join("replay.json"),out.join("provenance.json")];
                if mode=="counters" {
                    outputs.push(out.join("hardware-summary.json"));
                    invoke(o,python_script(python,&scripts.join("dashboard.py"),vec!["--profile".into(),profile.join("summary.json").display().to_string(),"--hardware".into(),out.join("hardware-summary.json").display().to_string(),"--output".into(),dir.join("dashboard").display().to_string()]),&dir.join("dashboard.log"))?;
                    outputs.push(dir.join("dashboard/index.html"));
                    label_dashboard(&dir.join("dashboard"),case)?;
                } else {ensure!(out.join("window-0/window.gputrace").is_dir(),"capture missing");outputs.extend(files_under(&out.join("window-0/window.gputrace"),&[])?);}
                Ok((json!({"output":out,"capture_is_profiled":false}),outputs))
            })?;
            extra[mode] = json!({"cache":stage_note(&entry),"result":entry.detail});
        }
    }
    Ok(
        json!({"name":case.name,"source_sha256":hash_bytes(&case.bytes),"marker_tables":compiled.detail["marker_tables"],"clean":baseline.detail,"encoder_profile":summary,"dashboard":view.detail["dashboard"],"cache":{"compile":stage_note(&compiled),"baseline":stage_note(&baseline),"encoders":stage_note(&measured),"view":stage_note(&view)},"optional":extra}),
    )
}

fn select_window(summary: &Value, dispatches: u64, count: u64) -> Result<(u64, u64)> {
    ensure!(dispatches > 0, "no command buffer to profile");
    let hot = summary["hot_commands"]
        .as_array()
        .context("no hotspot coordinates")?;
    let start = hot
        .iter()
        .filter_map(|c| c["outer_before"].as_u64())
        .find(|&i| i < dispatches)
        .context("no valid hotspot")?;
    Ok((start, count.min(dispatches - start)))
}
fn markdown(report: &Value) -> String {
    let mut s = format!(
        "# Eggfuse Pass 3 profile suite\n\nConfiguration: `{}`. Clean inference excludes compilation, setup and export. Cached rows retain their original measurement.\n\n| Case | Timing | Inference s | Physical rows | Runs | Private Runs | Source markers | Profile / clean |\n|---|---|---:|---:|---:|---:|---:|---:|\n",
        report["configuration"].as_str().unwrap_or("unknown")
    );
    for c in report["cases"].as_array().unwrap() {
        let p = &c["encoder_profile"];
        s += &format!(
            "| {} | {} | {:.6} | {} | {} | {} | {} | {:.2}x |\n",
            c["name"].as_str().unwrap(),
            if c["cache"]["baseline"]["cache_hit"] == true {
                "CACHED"
            } else {
                "NEW"
            },
            c["clean"]["inference_seconds"].as_f64().unwrap_or(0.0),
            c["clean"]["rows"],
            c["clean"]["runs"],
            c["clean"]["private_runs"],
            c["marker_tables"],
            p["timing_perturbation_ratio"].as_f64().unwrap_or(0.0)
        );
    }
    s += "\nEncoder percentages describe the instrumented run, not a clean-time decomposition.\n";
    for c in report["cases"].as_array().unwrap() {
        s += &format!(
            "\n- {}: {}\n",
            c["name"].as_str().unwrap(),
            c["dashboard"].as_str().unwrap()
        );
    }
    for f in report["failures"].as_array().unwrap() {
        s += &format!("\nFAIL {}: {}\n", f["case"], f["error"]);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hot_window_is_an_outer_command_not_a_logical_run() -> Result<()> {
        let p = json!({"hot_commands":[{"outer_before":9,"run_before":[2000]}]});
        assert_eq!(select_window(&p, 10, 16)?, (9, 1));
        assert!(select_window(&p, 0, 1).is_err());
        Ok(())
    }
}

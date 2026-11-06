#[cfg(not(feature = "allocation-census"))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;
#[cfg(feature = "allocation-census")]
#[global_allocator]
static CENSUS: eggfuse_cell_runtime::allocation_census::Allocator =
    eggfuse_cell_runtime::allocation_census::Allocator;
use eggfuse_cell_runtime::{executor::Executor, graph::Graph, program::Program};
use std::{path::PathBuf, time::Instant};
#[cfg(all(unix, feature = "storage-metrics"))]
fn peak_rss_bytes() -> Result<u64, Box<dyn std::error::Error>> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // getrusage initializes the output on success. Darwin reports bytes;
    // Linux reports KiB. Sample before building any export or report.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let peak = unsafe { usage.assume_init() }.ru_maxrss as u64;
    Ok(if cfg!(target_os = "macos") {
        peak
    } else {
        peak * 1024
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let opt = |k: &str| args.windows(2).find(|w| w[0] == k).map(|w| w[1].clone());
    if args.iter().any(|x| x == "--help") || args.is_empty() {
        println!(
            "eggfuse-cell-runtime --program FILE [--inspect-program | --output DIR] [--audit-stages] [--extract-unit-cost OPERATOR]"
        );
        #[cfg(feature = "structural-depth")]
        println!("  --structural-depth 0..3 [--structural-survey | --structural-catalog FILE]");
        #[cfg(feature = "rhs-locals")]
        println!("  --rhs-local-width N [--rhs-local-profile FILE] (0 disables local RHS blocks)");
        #[cfg(feature = "rhs-work-audit")]
        println!("  --rhs-cohort FILE (replay the recorded RHS work cohort)");
        #[cfg(feature = "entry-retirement")]
        println!("  --entry-retire-interval N (default 1)");
        println!("  --release-source-artifact (discard validation IR after runtime setup)");
        #[cfg(feature="port-frames")]
        println!("  --port-frames (compact checked constructor blocks) [--frame-census]");
        #[cfg(feature="two-run-blocks")]
        println!("  --two-run-blocks (execute checked two-phase port catalog)");
        #[cfg(feature="two-run-blocks")]
        println!("  --multi-run-blocks [--port-block-depth 2..8] (early shared input/result ports)");
        #[cfg(feature="two-run-blocks")]
        println!("  --block-lhs (use guarded block postconditions for residual matching)");
        #[cfg(feature="two-run-survey")]
        println!("  --two-run-survey (diagnostic owner-to-consumer census)");
        #[cfg(feature = "deferred-incidence")]
        println!("  --deferred-incidence off|edges|on (delay reverse edges and row payloads)");
        return Ok(());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut flags = args.iter();
    while let Some(flag) = flags.next() {
        if !seen.insert(flag) {
            return Err(format!("duplicate option {flag}").into());
        }
        match flag.as_str() {
            "--program" | "--output" | "--extract-unit-cost" => {
                flags
                    .next()
                    .filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| format!("missing value for {flag}"))?;
            }
            #[cfg(feature = "structural-depth")]
            "--structural-depth" | "--structural-catalog" => {
                flags
                    .next()
                    .filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| format!("missing value for {flag}"))?;
            }
            #[cfg(feature = "structural-depth")]
            "--structural-survey" => {}
            #[cfg(feature = "rhs-work-audit")]
            "--rhs-cohort" => {
                flags
                    .next()
                    .filter(|s| !s.starts_with("--"))
                    .ok_or("missing cohort path")?;
            }
            #[cfg(feature = "rhs-locals")]
            "--rhs-local-width" | "--rhs-local-profile" => {
                flags
                    .next()
                    .filter(|s| !s.starts_with("--"))
                    .ok_or("missing block width")?;
            }
            #[cfg(feature = "keeper-proof")]
            "--keeper-pairs" => {
                flags
                    .next()
                    .filter(|v| !v.starts_with("--"))
                    .ok_or("missing keeper pairs")?;
            }
            #[cfg(feature = "entry-retirement")]
            "--entry-retire-interval" => {
                flags
                    .next()
                    .filter(|v| !v.starts_with("--"))
                    .ok_or("missing retirement interval")?;
            }
            "--audit-stages" | "--inspect-program" | "--release-source-artifact" => {}
            #[cfg(feature = "port-frames")]
            "--frame-census" => {}
            #[cfg(feature = "port-frames")]
            "--port-frames" => {}
            #[cfg(feature="two-run-survey")]
            "--two-run-survey" => {}
            #[cfg(feature="two-run-blocks")]
            "--two-run-blocks" => {}
            #[cfg(feature="two-run-blocks")]
            "--multi-run-blocks"=>{},
            #[cfg(feature="two-run-blocks")]
            "--block-lhs"=>{},
            #[cfg(feature="two-run-blocks")]
            "--port-block-depth"=>{flags.next().ok_or("missing port block depth")?;},
            #[cfg(feature = "deferred-incidence")]
            "--deferred-incidence" => {
                let value = flags.next().ok_or("missing incidence mode")?;
                if !["off", "edges", "on"].contains(&value.as_str()) { return Err("incidence mode must be off, edges or on".into()); }
            }
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    #[cfg(feature = "allocation-census")]
    let loading = eggfuse_cell_runtime::allocation_census::scope(
        eggfuse_cell_runtime::allocation_census::Tag::Artifact,
    );
    let mut bytes = std::fs::read(opt("--program").ok_or("--program required")?)?;
    if args.iter().any(|s|s=="--two-run-survey") && args.iter().any(|s|s=="--two-run-blocks"){
        return Err("run the two-run survey separately from fusion".into());
    }
    let program = Program::parse(serde_json::from_slice(&bytes)?)?;
    #[cfg(feature = "allocation-census")]
    drop(loading);
    if args.iter().any(|x| x == "--inspect-program") {
        let (rules, orientations) = program.context_cut_counts();
        println!(
            "{}",
            serde_json::json!({"context_cut_rules":rules,"context_cut_orientations":orientations,"backend":"cell-cpu", "validated":true,
            })
        );
        return Ok(());
    }
    let out = PathBuf::from(opt("--output").ok_or("--output required")?);
    std::fs::create_dir(&out)?;
    let start = Instant::now();
    let audit = args.iter().any(|x| x == "--audit-stages");
    let mut runtime = Executor::new(Graph::new(program)?, audit);
    #[cfg(feature="two-run-blocks")]
    runtime.graph.configure_block_lhs(args.iter().any(|s|s=="--block-lhs"));
    #[cfg(feature = "port-frames")]
    runtime.graph.configure_frame_census(args.iter().any(|x|x=="--frame-census"));
    #[cfg(feature = "port-frames")]
    runtime.graph.configure_port_frames(args.iter().any(|x|x=="--port-frames"));
    #[cfg(feature="two-run-survey")]
    runtime.graph.configure_two_run_survey(args.iter().any(|x|x=="--two-run-survey"));
    #[cfg(feature="two-run-blocks")]
    runtime.configure_two_run(args.iter().any(|x|x=="--two-run-blocks"));
    #[cfg(feature="two-run-blocks")]
    if args.iter().any(|x|x=="--multi-run-blocks") {
        if args.iter().any(|x|x=="--two-run-blocks" || x=="--two-run-survey") {return Err("choose legacy fusion, multi-run fusion, or survey".into())}
        runtime.configure_multi_run(opt("--port-block-depth").map(|s|s.parse()).transpose()?.unwrap_or(8))?;
    }
    #[cfg(feature = "deferred-incidence")]
    {
        let mode = opt("--deferred-incidence").unwrap_or_else(|| "off".into());
        runtime.graph.configure_deferred_incidence(mode != "off");
        runtime.graph.configure_deferred_payloads(mode == "on");
    }
    #[cfg(feature = "structural-depth")]
    {
        let survey = args.iter().any(|x| x == "--structural-survey");
        let depth = opt("--structural-depth")
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or(if survey { 3 } else { 0 });
        runtime.configure_depth(depth, survey)?;
        if let Some(path) = opt("--structural-catalog") {
            runtime.use_depth_catalog(serde_json::from_reader(std::fs::File::open(path)?)?)?;
        }
    }
    #[cfg(feature = "rhs-work-audit")]
    if let Some(path) = opt("--rhs-cohort") {
        runtime.rhs_cohort(std::path::Path::new(&path))?;
    }
    #[cfg(feature = "rhs-locals")]
    runtime.configure_rhs_locals(
        opt("--rhs-local-width")
            .map(|v| v.parse())
            .transpose()?
            .unwrap_or(0),
    )?;
    #[cfg(feature = "rhs-locals")]
    if let Some(path) = opt("--rhs-local-profile") {
        runtime.use_rhs_local_profile(&serde_json::from_reader(std::fs::File::open(path)?)?)?;
    }
    #[cfg(feature = "keeper-proof")]
    if let Some(path) = opt("--keeper-pairs") {
        runtime.read_keepers(std::path::Path::new(&path))?;
    }
    #[cfg(feature = "entry-retirement")]
    runtime.configure_entry_retirement(
        opt("--entry-retire-interval")
            .map(|v| v.parse())
            .transpose()?
            .unwrap_or(1),
    )?;
    if args.iter().any(|a| a == "--release-source-artifact") {
        runtime.release_validation_artifact()?;
        drop(std::mem::take(&mut bytes));
    }
    #[cfg(all(unix, feature = "storage-metrics"))]
    let setup_peak_rss_bytes = peak_rss_bytes()?;
    #[cfg(feature = "allocation-census")]
    let memory_setup = eggfuse_cell_runtime::allocation_census::snapshot();
    let setup = start.elapsed().as_secs_f64();
    let start = Instant::now();
    #[cfg(feature = "slot-transfer-audit")]
    {
        eggfuse_cell_runtime::transfer_audit::set_active(true);
        eggfuse_cell_runtime::transfer_audit::enable_write_audit();
    }
    if let Err(error) = runtime.execute() {
        std::fs::write(
            out.join("failure.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"error":error,"status":"error","completed_steps":runtime.steps}),
            )?,
        )?;
        return Err(error.into());
    }
    #[cfg(feature = "slot-transfer-audit")]
    {
        eggfuse_cell_runtime::transfer_audit::set_active(false);
        std::fs::write(
            out.join("resumes.json"),
            serde_json::to_vec(&eggfuse_cell_runtime::transfer_audit::take_resume_digests())?,
        )?;
        std::fs::write(
            out.join("writes.json"),
            serde_json::to_vec(&eggfuse_cell_runtime::transfer_audit::take_write_digests())?,
        )?;
        std::fs::write(
            out.join("rhs.json"),
            serde_json::to_vec(&eggfuse_cell_runtime::transfer_audit::take_digests())?,
        )?;
    }
    let seconds = start.elapsed().as_secs_f64();
    #[cfg(feature = "allocation-census")]
    let memory_inference = eggfuse_cell_runtime::allocation_census::snapshot();
    #[cfg(all(unix, feature = "storage-metrics"))]
    let inference_peak_rss_bytes = peak_rss_bytes()?;
    #[cfg(feature = "spine-shapes")]
    std::fs::write(
        out.join("spine-shapes.json"),
        serde_json::to_vec(&runtime.spine_shape_report())?,
    )?;
    #[cfg(feature = "family-survey")]
    std::fs::write(
        out.join("family-survey.json"),
        serde_json::to_vec(&runtime.family_survey())?,
    )?;
    #[cfg(feature = "structural-depth")]
    {
        let report = runtime.depth_report();
        if !report["catalog"].is_null() {
            std::fs::write(
                out.join("structural-catalog.json"),
                serde_json::to_vec_pretty(&report["catalog"])?,
            )?;
        }
        std::fs::write(
            out.join("structural-depth.json"),
            serde_json::to_vec(&report)?,
        )?;
    }
    #[cfg(feature = "rhs-work-audit")]
    runtime.write_rhs_work(&out)?;
    #[cfg(feature = "keeper-proof")]
    runtime.write_keepers(&out)?;
    let mut report = runtime.report();
    #[cfg(all(feature="two-run-blocks",feature="slot-transfer-audit"))]
    if args.iter().any(|s|s=="--block-lhs") {
        std::fs::write(out.join("block-lhs-plans.json"),serde_json::to_vec(&runtime.graph.block_lhs_plans())?)?;
    }
    #[cfg(feature="two-run-blocks")]
    {report["block_lhs"]=runtime.graph.block_lhs_report();}
    #[cfg(feature = "port-frames")]
    { report["frame_census"] = runtime.graph.frame_census_report(); }
    #[cfg(feature = "port-frames")]
    { report["port_frames"] = runtime.graph.port_frames_report(); }
    #[cfg(feature="two-run-blocks")]
    {report["two_run_blocks"]=runtime.graph.two_run_report();}
    #[cfg(feature = "allocation-census")]
    {
        report["allocation_census"] =
            serde_json::json!({"setup":memory_setup,"inference":memory_inference});
    }
    #[cfg(all(unix, feature = "storage-metrics"))]
    {
        report["peak_rss_through_setup_bytes"] = setup_peak_rss_bytes.into();
        report["peak_rss_through_inference_bytes"] = inference_peak_rss_bytes.into();
        report["storage"] = runtime.graph.storage_sample();
    }
    #[cfg(feature = "rhs-locals")]
    {
        report["rhs_locals"] = runtime.rhs_local_report();
    }
    report["inference_seconds"] = seconds.into();
    report["setup_seconds"] = setup.into();
    let export_start = Instant::now();
    let mut writer = std::io::BufWriter::new(std::fs::File::create(out.join("state.json"))?);
    runtime.graph.write_export(&mut writer)?;
    std::io::Write::flush(&mut writer)?;
    report["export_seconds"] = export_start.elapsed().as_secs_f64().into();
    if let Some(op) = opt("--extract-unit-cost") {
        std::fs::write(
            out.join("extraction.json"),
            serde_json::to_vec_pretty(&runtime.graph.extract_unit_cost(&op)?)?,
        )?;
    }
    if runtime.audit {
        std::fs::write(
            out.join("stages.json"),
            serde_json::to_vec(&runtime.snapshots)?,
        )?;
        std::fs::write(
            out.join("binding-audit.json"),
            serde_json::to_vec(
                &serde_json::json!({"initial":runtime.initial_audit,"runs":runtime.binding_audit}),
            )?,
        )?;
    }
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{}",
        serde_json::json!({"status":report["status"],"inference_seconds":seconds,"live_cells":runtime.graph.cells.len(),"stats":runtime.graph.stats})
    );
    #[cfg(feature = "allocation-census")]
    {
        let before = eggfuse_cell_runtime::allocation_census::snapshot();
        drop(bytes);
        let after_input = eggfuse_cell_runtime::allocation_census::snapshot();
        drop(std::mem::take(&mut runtime.graph.program.artifact));
        let after_artifact = eggfuse_cell_runtime::allocation_census::snapshot();
        drop(runtime);
        let after_engine = eggfuse_cell_runtime::allocation_census::snapshot();
        std::fs::write(
            out.join("allocation-teardown.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"before":before,"after_input":after_input,"after_artifact":after_artifact,"after_engine":after_engine}),
            )?,
        )?;
    }
    Ok(())
}

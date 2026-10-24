//! Execute one self-contained source. Cell plans are declared in its comments;
//! no application-specific setup rewrite or external plan is loaded.
use crate::{Result, program_io as io};
use egglog::{EGraph, ast::Command};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, time::Instant};
fn required<'a>(a: &'a BTreeMap<String, String>, k: &str) -> Result<&'a str> {
    Ok(a.get(k).ok_or_else(|| format!("missing {k}"))?.as_str())
}
pub fn run(a: &BTreeMap<String, String>) -> Result<()> {
    for key in a.keys() {
        if ![
            "--mode",
            "--backend",
            "--source",
            "--output",
            "--pristine-runner",
            "--gpu-worker",
            "--query-audit",
            "--native-rulesets",
            "--cell-assistance",
            "--export-extraction",
        ]
        .contains(&key.as_str())
        {
            return Err(format!("unsupported program-mode option {key}; assistance and source transforms are not implicit").into());
        }
    }
    if a.get("--query-audit")
        .is_some_and(|s| s != "on" && s != "off")
    {
        return Err("--query-audit expects on/off".into());
    }
    let source = Path::new(required(a, "--source")?);
    let out = Path::new(required(a, "--output")?);
    let assistance = a
        .get("--cell-assistance")
        .map(String::as_str)
        .unwrap_or("auto");
    if !["auto", "on", "off"].contains(&assistance) {
        return Err("cell-assistance expects auto/on/off".into());
    }
    for key in ["CELL_BOUNDARY_PROGRAMS", "CELL_CERTIFICATE_LIBRARY"] {
        if std::env::var_os(key).is_some() {
            return Err(format!(
                "{key} is not an input to program mode; embed the plan in source_sk_mk.egg"
            )
            .into());
        }
    }
    let export = match a
        .get("--export-extraction")
        .map(String::as_str)
        .unwrap_or("off")
    {
        "on" => true,
        "off" => false,
        _ => return Err("export-extraction expects on/off".into()),
    };
    EGraph::set_num_threads(1);
    // These are timing/execution lanes, not native join-counter profiles.
    // EGGLOG_PROBE_COUNTS_OFF is not a native API; use the actual toggle.
    egglog::set_probe_counts_enabled(false);
    match required(a, "--backend")? {
        "inspect" => {
            fn closed(c: &Command) -> bool {
                match c {
                    Command::Include(..) | Command::Input { .. } | Command::Output { .. } => false,
                    Command::Fail(_, child) => closed(child),
                    _ => true,
                }
            }
            let mut parser = EGraph::default();
            let commands = parser.parse_program(
                Some(source.display().to_string()),
                &std::fs::read_to_string(source)?,
            )?;
            println!("{}", json!({"cache_safe":commands.iter().all(closed)}));
            Ok(())
        }

        "native" => io::native(
            source,
            out,
            "native-fork",
            json!({"kernel":"current local fork; unassisted native execution"}),
        ),
        "pristine" => {
            let binary = Path::new(required(a, "--pristine-runner")?);
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(binary.with_extension("json"))?)?;
            let digest = std::process::Command::new("shasum")
                .args(["-a", "256"])
                .arg(binary)
                .output()?;
            if !digest.status.success()
                || std::str::from_utf8(&digest.stdout)?
                    .split_whitespace()
                    .next()
                    != manifest["binary_sha256"].as_str()
            {
                return Err("pristine binary fingerprint mismatch".into());
            }
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
            let tag = std::process::Command::new("git")
                .args(["rev-parse", "egglog-baseline:egglog"])
                .current_dir(root)
                .output()?;
            if !tag.status.success()
                || std::str::from_utf8(&tag.stdout)?.trim()
                    != manifest["kernel_tree"].as_str().unwrap_or("")
            {
                return Err("pristine kernel does not match immutable baseline".into());
            }
            let status = std::process::Command::new(binary)
                .arg(source)
                .arg(out)
                .status()?;
            if !status.success() {
                return Err("pristine program failed; report retained".into());
            }
            Ok(())
        }
        "gpu" | "cpu" => gpu(
            source,
            out,
            if required(a, "--backend")? == "gpu" {
                Some(Path::new(required(a, "--gpu-worker")?))
            } else {
                None
            },
            a.get("--query-audit").is_some_and(|s| s == "on"),
            a.get("--native-rulesets").map(String::as_str).unwrap_or(""),
            assistance,
            export,
        ),
        _ => Err("program backend must be pristine, native, cpu or gpu".into()),
    }
}
fn gpu(
    source: &Path,
    out: &Path,
    worker: Option<&Path>,
    audit: bool,
    native: &str,
    assistance: &str,
    export: bool,
) -> Result<()> {
    std::fs::create_dir(out)?;
    let backend = if worker.is_some() { "gpu" } else { "cpu" };
    let text = std::fs::read_to_string(source)?;
    let (_, all, _) = io::load(source)?;
    let document = crate::source_cells::Document::parse(&text, &all)?;
    if assistance == "on" && document.is_none() {
        return Err("no Cell declarations in source; run the marker stage first".into());
    }
    let assisted = assistance != "off" && document.is_some();
    let prepared = io::prepare(source).and_then(|(eg, c, s)| {
        let (eg, h) = if assisted {
            document.as_ref().unwrap().attach(eg)?
        } else {
            (eg, None)
        };
        Ok((eg, c, s, h))
    });
    let (frozen, commands, setup, hook) = match prepared {
        Ok(x) => x,
        Err(e) => {
            std::fs::write(
                out.join("report.json"),
                serde_json::to_vec_pretty(
                    &json!({"backend":backend,"complete":false,"phase":"setup","error":e.to_string()}),
                )?,
            )?;
            return Err(e);
        }
    };
    let schedules = commands
        .iter()
        .filter(|c| matches!(c, Command::RunSchedule(_)))
        .cloned()
        .collect::<Vec<_>>();
    let start = Instant::now();
    fn collect(s: &egglog::ast::Schedule, out: &mut std::collections::BTreeSet<String>) {
        use egglog::ast::GenericSchedule::*;
        match s {
            Run(_, r) => {
                out.insert(r.ruleset.clone());
            }
            Sequence(_, xs) => {
                for x in xs {
                    collect(x, out)
                }
            }
            Repeat(_, _, s) | Saturate(_, s) => collect(s, out),
        }
    }
    let mut native = native
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(d) = &document {
        native.extend(d.native_rulesets.iter().cloned());
    }
    // The preceding key stage explicitly declares its helper execution sets.
    for line in text
        .lines()
        .filter_map(|s| s.strip_prefix(crate::stable_choice::source::HEADER))
    {
        let spec: serde_json::Value = serde_json::from_str(line)?;
        for s in spec["native_rulesets"].as_array().into_iter().flatten() {
            native.insert(
                s.as_str()
                    .ok_or("invalid source native ruleset")?
                    .to_owned(),
            );
        }
    }
    let mut sets = std::collections::BTreeSet::new();
    for c in &schedules {
        if let Command::RunSchedule(s) = c {
            collect(s, &mut sets)
        }
    }
    // Helpers may already have run during native initialization. Validate names
    // against installed sets, not just the final/main schedule.
    for name in &native {
        frozen
            .cell_ruleset_members(name)
            .map_err(|_| format!("unknown explicit native ruleset {name:?}"))?;
    }
    for set in &sets {
        if !native.contains(set.as_str())
            && frozen
                .cell_ruleset_members(set)?
                .iter()
                .any(|(owner, _)| native.contains(owner.as_str()))
        {
            return Err(format!("cannot exclude a member of combined ruleset {set:?}; select the whole scheduled set as native").into());
        }
    }
    let selected = sets
        .into_iter()
        .filter(|s| !native.contains(s.as_str()))
        .collect::<Vec<_>>();
    let admitted = crate::table_live::Runtime::new_full_configured(
        frozen.clone(),
        &schedules,
        worker,
        audit,
        if native.is_empty() {
            None
        } else {
            Some(&selected)
        },
        hook,
        Default::default(),
    );
    let mut runtime = match admitted {
        Ok(r) => r,
        Err(e) => {
            io::save(
                out,
                &frozen,
                json!({"backend":backend,"complete":false,"phase":"gpu_setup","error":e.to_string(),"setup":setup}),
            )?;
            return Err(e);
        }
    };
    let device_setup = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let mut outputs = vec![];
    let result = (|| -> Result<()> {
        for c in commands {
            if matches!(&c, Command::RunSchedule(_)) {
                runtime.run(&[c])?;
            } else {
                outputs.extend(
                    runtime
                        .input_commands(vec![c])?
                        .iter()
                        .map(ToString::to_string),
                );
            }
        }
        Ok(())
    })();
    let inference_seconds = start.elapsed().as_secs_f64();
    let telemetry = runtime.report();
    let eg = runtime.into_engine();
    io::save(
        out,
        &eg,
        json!({"schema":"program-run/v1","backend":backend,"complete":result.is_ok(),
        "phase":"run","error":result.as_ref().err().map(ToString::to_string),"setup":setup,
        "run_seconds":inference_seconds,"gpu_setup_seconds":device_setup,
        "runtime":telemetry,"cell_assistance":assisted,"query_audit":audit,"native_probe_counters":false,"outputs":outputs,"explicit_native_rulesets":native,
        "source_rewritten_by_runner":false,"provenance":{"kernel":"current fork, same-submit queries with native effects"}}),
    )?;
    if let Some(d) = &document {
        let mut cells = d.report(&eg);
        cells["runtime"] = telemetry["saturation_diagnostics"].clone();
        cells["assistance_enabled"] = json!(assisted);
        std::fs::write(out.join("cells.json"), serde_json::to_vec_pretty(&cells)?)?;
    }
    if export {
        let graph = eg.serialize(egglog::SerializeConfig::default());
        if !graph.is_complete() {
            return Err("incomplete extraction graph".into());
        }
        serde_json::to_writer_pretty(
            std::io::BufWriter::new(std::fs::File::create(out.join("extraction-input.json"))?),
            &graph.egraph,
        )?;
    }
    result?;
    Ok(())
}

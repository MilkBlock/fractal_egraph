//! Same source, independently selected execution backend. No implicit stable
//! choice rewrite, Bake, Cell observer, source markers or shader CPU fallback.
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
    EGraph::set_num_threads(1);
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
        "gpu" => gpu(
            source,
            out,
            Path::new(required(a, "--gpu-worker")?),
            a.get("--query-audit").is_some_and(|s| s == "on"),
        ),
        _ => Err("program backend must be pristine, native or gpu".into()),
    }
}
fn gpu(source: &Path, out: &Path, worker: &Path, audit: bool) -> Result<()> {
    std::fs::create_dir(out)?;
    let (frozen, commands, setup) = match io::prepare(source) {
        Ok(x) => x,
        Err(e) => {
            std::fs::write(
                out.join("report.json"),
                serde_json::to_vec_pretty(
                    &json!({"backend":"gpu","complete":false,"phase":"setup","error":e.to_string()}),
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
    let admitted =
        crate::table_live::Runtime::new_full(frozen.clone(), &schedules, Some(worker), audit, None);
    let mut runtime = match admitted {
        Ok(r) => r,
        Err(e) => {
            io::save(
                out,
                &frozen,
                json!({"backend":"gpu","complete":false,"phase":"gpu_setup","error":e.to_string(),"setup":setup}),
            )?;
            return Err(e);
        }
    };
    let device_setup = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let result = runtime.run(&schedules);
    let telemetry = runtime.report();
    let mut eg = runtime.into_engine();
    // These commands follow the last schedule, so no future GPU frontier can
    // observe their mutations. Execute their original AST, including fail,
    // extraction, stack restoration and checks, without print/reparse.
    let tail = commands
        .into_iter()
        .filter(|c| !matches!(c, Command::RunSchedule(_)))
        .collect();
    let result = result.and_then(|_| Ok(eg.run_program(tail)?));
    let outputs = result
        .as_ref()
        .ok()
        .map(|xs| xs.iter().map(ToString::to_string).collect::<Vec<_>>());
    io::save(
        out,
        &eg,
        json!({"schema":"program-run/v1","backend":"gpu","complete":result.is_ok(),
        "phase":"run","error":result.as_ref().err().map(ToString::to_string),"setup":setup,
        "run_seconds":start.elapsed().as_secs_f64(),"gpu_setup_seconds":device_setup,
        "runtime":telemetry,"cell_assistance":false,"query_audit":audit,"outputs":outputs,
        "source_rewritten_by_runner":false,"provenance":{"kernel":"current fork, GPU queries with native effects"}}),
    )?;
    result?;
    Ok(())
}

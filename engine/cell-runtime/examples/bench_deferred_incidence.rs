//! Fresh-process off/on measurements on the two frozen programs. Diagnostic
//! binaries use the same driver with zero timed rounds and --audit.
use serde_json::{Value as J, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

fn hash(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    (xs[(xs.len() - 1) / 2] + xs[xs.len() / 2]) / 2.
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 5 || args.len() > 6 {
        return Err("expected BINARY NEW_OUTPUT SUM_PROGRAM RAY_PROGRAM ROUNDS [--audit]".into());
    }
    let binary = fs::canonicalize(&args[0])?;
    let output = PathBuf::from(&args[1]);
    fs::create_dir(&output)?;
    let rounds: usize = args[4].parse()?;
    let scheduled=args.get(5).is_some_and(|s|s=="--scheduled" || s=="--audit-scheduled");
    let functions=args.get(5).is_some_and(|s|s=="--functions" || s=="--audit-functions");
    let fusion = args
        .get(5)
        .is_some_and(|s| s == "--two-run" || s == "--audit-two-run");
    let audit = args
        .get(5)
        .is_some_and(|s| s == "--audit" || s == "--audit-ports" || s == "--audit-two-run" || s=="--audit-functions" || s=="--audit-scheduled");
    let ports = scheduled || functions || fusion
        || args
            .get(5)
            .is_some_and(|s| s == "--ports" || s == "--audit-ports");
    if rounds == 0 && !audit {
        return Err("zero rounds only for audit".into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let gold: J = serde_json::from_slice(&fs::read(root.join("docs/view-consumer-results.json"))?)?;
    let mut log = fs::File::create(output.join("samples.jsonl"))?;
    let mut samples = vec![];
    for (case, input) in [("sum-bits", &args[2]), ("raytrace", &args[3])] {
        let mut original: J = serde_json::from_slice(&fs::read(input)?)?;
        let mut admitted = original
            .as_object_mut()
            .unwrap()
            .remove("deferred_incidence_admission")
            .is_some();
        admitted |= original
            .as_object_mut()
            .unwrap()
            .remove("two_run_blocks")
            .is_some();
        original.as_object_mut().unwrap().remove("sha256");
        let original_sha = eggfuse_cell_runtime::checksum::artifact_digest(&original);
        for round in 0..=rounds {
            let mut modes = if ports {
                vec!["off", "on"]
            } else {
                vec!["off", "edges", "on"]
            };
            let n = modes.len();
            modes.rotate_left(round % n);
            for mode in modes {
                let run = output.join(format!("{case}-{round}-{mode}"));
                let mut cmd = Command::new(&binary);
                cmd.args(["--program", input, "--output"])
                    .arg(&run)
                    .args(["--deferred-incidence", if ports { "off" } else { mode }]);
                if scheduled || functions || fusion || (ports && mode == "on") {
                    cmd.arg("--port-frames");
                }
                if functions && mode=="on" {cmd.arg("--function-blocks");}
                if scheduled && mode=="on" {cmd.arg("--scheduled-functions");}
                if fusion && mode == "on" {
                    cmd.arg("--two-run-blocks");
                }
                for (k, _) in std::env::vars() {
                    if k.starts_with("CELL_CPU_") || k.starts_with("RAYON_") {
                        cmd.env_remove(k);
                    }
                }
                cmd.env("RAYON_NUM_THREADS", "1");
                let start = Instant::now();
                let result = cmd.output()?;
                let cli_ms = start.elapsed().as_secs_f64() * 1000.;
                if !result.status.success() {
                    return Err(String::from_utf8_lossy(&result.stderr).into_owned().into());
                }
                let report: J = serde_json::from_slice(&fs::read(run.join("report.json"))?)?;
                let state_hash = hash(&run.join("state.json"))?;
                assert_eq!(
                    state_hash,
                    gold["audits"][case]["state_sha256"].as_str().unwrap()
                );
                let fields = [
                    "ordinal",
                    "ruleset",
                    "path",
                    "iterations",
                    "epoch",
                    "logical_matches",
                    "executed_bindings",
                    "changed",
                ];
                let steps = report["steps"].as_array().unwrap();
                let schedule = json!(
                    steps
                        .iter()
                        .map(|s| fields.iter().map(|k| s[*k].clone()).collect::<Vec<_>>())
                        .collect::<Vec<_>>()
                );
                let schedule_hash = format!(
                    "{:x}",
                    Sha256::digest(
                        eggfuse_cell_runtime::checksum::canonical(&schedule, true).as_bytes()
                    )
                );
                assert_eq!(
                    schedule_hash,
                    gold["audits"][case]["schedule_sha256"].as_str().unwrap()
                );
                let mut digests = json!({"state":state_hash,"schedule":schedule_hash});
                if audit {
                    for (name, expected) in gold["audits"][case]["digests"].as_object().unwrap() {
                        let path = run.join(format!("{name}.json"));
                        let mut got = hash(&path)?;
                        if name == "spine-shapes" && admitted {
                            digests["spine-shapes_raw"] = json!(got);
                            let mut shape: J = serde_json::from_slice(&fs::read(&path)?)?;
                            assert_eq!(shape["program_sha256"], report["program_sha256"]);
                            // Only the admission hint changes artifact identity;
                            // compare every static entry and trajectory unchanged.
                            shape["program_sha256"] = json!(original_sha);
                            got = format!("{:x}", Sha256::digest(serde_json::to_vec(&shape)?));
                        }
                        assert_eq!(got, expected.as_str().unwrap(), "{case} {mode} {name}");
                        digests[name] = json!(got);
                    }
                }
                let mut phases = json!({});
                for name in ["search", "rhs", "repair", "delivery_gc", "query_update"] {
                    phases[name] = json!(
                        steps
                            .iter()
                            .map(|s| s["phases"][name].as_f64().unwrap())
                            .sum::<f64>()
                            * 1000.
                    );
                }
                let sample = json!({"case":case,"round":round,"mode":mode,"audit":audit,
                    "inference_ms":report["inference_seconds"].as_f64().unwrap()*1000.,"cli_ms":cli_ms,
                    "rss_bytes":report["peak_rss_through_inference_bytes"],"phases_ms":phases,
                    "stats":report["stats"],"deferred_incidence":report["deferred_incidence"],"port_frames":report["port_frames"],"two_run_blocks":report["two_run_blocks"],"storage":report["storage"],
                    "function_blocks":report["function_blocks"],"scheduled_functions":report["scheduled_functions"],"publication":report["publication"],"entry_schedule":report["entry_schedule"],"sieve":report["sieve"],"indexed_columns":report["indexed_columns"],"spine":report["stage_spine"]["stats"],"runs":steps.len(),"digests":digests});
                writeln!(log, "{sample}")?;
                log.flush()?;
                println!(
                    "{case} {mode} round {round}: {} ms; deferred {}",
                    sample["inference_ms"],
                    sample["deferred_incidence"]["stats"]["deferred_births"]
                );
                samples.push(sample);
                // Only this freshly created child output is disposable; failed
                // executions and failed checks above retain their complete data.
                fs::remove_dir_all(&run)?;
            }
        }
    }
    let mut summary = json!({"binary_sha256":hash(&binary)?,"rounds":rounds,"audit":audit,
        "inputs":{"sum-bits":hash(Path::new(&args[2]))?,"raytrace":hash(Path::new(&args[3]))?},"cases":{}});
    if rounds > 0 {
        for case in ["sum-bits", "raytrace"] {
            for mode in if ports {
                vec!["off", "on"]
            } else {
                vec!["off", "edges", "on"]
            } {
                let selected: Vec<_> = samples
                    .iter()
                    .filter(|s| {
                        s["case"] == case && s["mode"] == mode && s["round"].as_u64().unwrap() > 0
                    })
                    .collect();
                let mut values = json!({});
                for key in ["inference_ms", "cli_ms", "rss_bytes"] {
                    values[key] = json!(median(
                        selected.iter().map(|s| s[key].as_f64().unwrap()).collect()
                    ));
                }
                values["deferred_incidence"] = selected[0]["deferred_incidence"].clone();
                values["port_frames"] = selected[0]["port_frames"].clone();
                values["storage"] = selected[0]["storage"].clone();
                values["function_blocks"] = selected[0]["function_blocks"].clone();
                for key in ["scheduled_functions","publication","entry_schedule","sieve","indexed_columns"] {values[key]=selected[0][key].clone();}
                for key in ["search","rhs","repair","delivery_gc","query_update"] {
                    values["phases_ms"][key]=json!(median(selected.iter().map(|s|s["phases_ms"][key].as_f64().unwrap()).collect()));
                }
                summary["cases"][case][mode] = values;
            }
        }
    }
    fs::write(
        output.join("summary.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    Ok(())
}
